from __future__ import annotations

import os
import re
from contextlib import asynccontextmanager
from dataclasses import dataclass
from datetime import date, datetime, time, timedelta, timezone
from decimal import Decimal, InvalidOperation
from typing import Any, AsyncIterator
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

import asyncpg
from dotenv import load_dotenv
from mcp.server.fastmcp import Context, FastMCP


load_dotenv()

DATE_ONLY_RE = re.compile(r"^\d{4}-\d{2}-\d{2}$")

MAX_LIMIT = 100
DEFAULT_LIMIT = 10
MAX_COST = Decimal("999999.99")

SUPPORTED_TRANSPORTS = {"stdio", "streamable-http", "sse"}


@dataclass(frozen=True)
class AppContext:
    pool: asyncpg.Pool
    local_timezone: ZoneInfo


def _env_int(name: str, default: int) -> int:
    value = os.getenv(name)
    if not value:
        return default

    try:
        parsed = int(value)
    except ValueError as exc:
        raise RuntimeError(f"{name} must be an integer") from exc

    if parsed < 1:
        raise RuntimeError(f"{name} must be greater than 0")

    return parsed


def _env_float(name: str, default: float) -> float:
    value = os.getenv(name)
    if not value:
        return default

    try:
        parsed = float(value)
    except ValueError as exc:
        raise RuntimeError(f"{name} must be a number") from exc

    if parsed <= 0:
        raise RuntimeError(f"{name} must be greater than 0")

    return parsed


def _load_local_timezone() -> ZoneInfo:
    timezone_name = os.getenv("LOCAL_TIMEZONE") or os.getenv("TZ") or "Asia/Shanghai"

    try:
        return ZoneInfo(timezone_name)
    except ZoneInfoNotFoundError as exc:
        raise RuntimeError(f"Unknown timezone: {timezone_name}") from exc


@asynccontextmanager
async def app_lifespan(_: FastMCP) -> AsyncIterator[AppContext]:
    database_url = os.getenv("DATABASE_URL")
    if not database_url:
        raise RuntimeError("DATABASE_URL is required")

    pool = await asyncpg.create_pool(
        dsn=database_url,
        min_size=_env_int("DATABASE_POOL_MIN_SIZE", 1),
        max_size=_env_int("DATABASE_POOL_MAX_SIZE", 5),
        command_timeout=_env_float("DATABASE_COMMAND_TIMEOUT", 10.0),
    )

    try:
        yield AppContext(
            pool=pool,
            local_timezone=_load_local_timezone(),
        )
    finally:
        await pool.close()


mcp = FastMCP(
    name="teslamate-charging-cost-mcp",
    lifespan=app_lifespan,
    stateless_http=True,
    json_response=True,
    host=os.getenv("MCP_HOST", "0.0.0.0"),
    port=_env_int("MCP_PORT", 8000),
    instructions=(
        "TeslaMate 充电费用助手。"
        "可以查询 TeslaMate 充电记录，查看充电详情，"
        "筛选未填写费用的记录，并手动更新公共充电站实际费用。"
        "费用写入 charging_processes.cost。"
        "TeslaMate 本身不存储货币类型，currency 只作为返回说明。"
    ),
)


def _app_context(ctx: Context) -> AppContext:
    return ctx.request_context.lifespan_context


def _normalize_limit(limit: int | None, default: int = DEFAULT_LIMIT) -> int:
    if limit is None:
        return default

    try:
        parsed = int(limit)
    except (TypeError, ValueError) as exc:
        raise ValueError("limit must be an integer") from exc

    if parsed < 1:
        raise ValueError("limit must be greater than 0")

    return min(parsed, MAX_LIMIT)


def _normalize_cost(cost: int | float | str) -> Decimal:
    try:
        value = Decimal(str(cost)).quantize(Decimal("0.01"))
    except (InvalidOperation, ValueError) as exc:
        raise ValueError("cost must be a valid number") from exc

    if value < 0:
        raise ValueError("cost must be greater than or equal to 0")

    if value > MAX_COST:
        raise ValueError(f"cost must be less than or equal to {MAX_COST}")

    return value


def _format_decimal(value: Any) -> str | None:
    if value is None:
        return None

    return str(Decimal(str(value)).quantize(Decimal("0.01")))


def _as_utc(value: datetime | None) -> datetime | None:
    if value is None:
        return None

    if value.tzinfo is None:
        return value.replace(tzinfo=timezone.utc)

    return value.astimezone(timezone.utc)


def _format_datetime(
    value: datetime | None,
    local_timezone: ZoneInfo,
) -> dict[str, str | None]:
    utc_value = _as_utc(value)

    if utc_value is None:
        return {
            "utc": None,
            "local": None,
        }

    return {
        "utc": utc_value.isoformat().replace("+00:00", "Z"),
        "local": utc_value.astimezone(local_timezone).isoformat(),
    }


def _parse_date_boundary(
    raw_value: str,
    *,
    local_timezone: ZoneInfo,
    end_boundary: bool,
) -> datetime:
    value = raw_value.strip()

    if not value:
        raise ValueError("date values cannot be empty")

    if DATE_ONLY_RE.match(value):
        parsed_date = date.fromisoformat(value)

        if end_boundary:
            parsed_date += timedelta(days=1)

        local_value = datetime.combine(
            parsed_date,
            time.min,
            tzinfo=local_timezone,
        )
    else:
        normalized = value.replace("Z", "+00:00")

        try:
            local_value = datetime.fromisoformat(normalized)
        except ValueError as exc:
            raise ValueError(
                "dates must use ISO format, e.g. 2026-05-03 "
                "or 2026-05-03T10:30:00"
            ) from exc

        if local_value.tzinfo is None:
            local_value = local_value.replace(tzinfo=local_timezone)

    return local_value.astimezone(timezone.utc).replace(tzinfo=None)


def _location_from_record(record: asyncpg.Record) -> str:
    return (
        record["geofence_name"]
        or record["address"]
        or "未知地点"
    )


def _charge_payload(
    record: asyncpg.Record,
    local_timezone: ZoneInfo,
    ) -> dict[str, Any]:
    start_date = _format_datetime(record["start_date"], local_timezone)
    end_date = _format_datetime(record["end_date"], local_timezone)

    location = _location_from_record(record)

    range_gained_km = None
    if (
        record["start_ideal_range_km"] is not None
        and record["end_ideal_range_km"] is not None
    ):
        range_gained_km = float(
            record["end_ideal_range_km"] - record["start_ideal_range_km"]
        )

    return {
        "id": record["id"],
        "car_id": record["car_id"],
        "car_name": record["car_name"],
        "start_date": start_date,
        "end_date": end_date,
        "duration_min": record["duration_min"],
        "location": location,
        "geofence_id": record["geofence_id"],
        "geofence_name": record["geofence_name"],
        "address_id": record["address_id"],
        "address": record["address"],
        "position_id": record["position_id"],
        "start_battery_level": record["start_battery_level"],
        "end_battery_level": record["end_battery_level"],
        "start_ideal_range_km": (
            float(record["start_ideal_range_km"])
            if record["start_ideal_range_km"] is not None
            else None
        ),
        "end_ideal_range_km": (
            float(record["end_ideal_range_km"])
            if record["end_ideal_range_km"] is not None
            else None
        ),
        "range_gained_km": range_gained_km,
        "charge_energy_added_kwh": (
            float(record["charge_energy_added"])
            if record["charge_energy_added"] is not None
            else None
        ),
        "charge_energy_used_kwh": (
            float(record["charge_energy_used"])
            if record["charge_energy_used"] is not None
            else None
        ),
        "cost": _format_decimal(record["cost"]),
        "has_cost": record["cost"] is not None,
    }


BASE_CHARGE_SELECT = """
    SELECT
        cp.id,
        cp.car_id,
        c.name AS car_name,
        cp.start_date,
        cp.end_date,
        cp.duration_min,
        cp.start_battery_level,
        cp.end_battery_level,
        cp.start_ideal_range_km,
        cp.end_ideal_range_km,
        cp.charge_energy_added,
        cp.charge_energy_used,
        cp.cost,
        cp.geofence_id,
        geo.name AS geofence_name,
        cp.address_id,
        addr.display_name AS address,
        cp.position_id
    FROM charging_processes AS cp
    LEFT JOIN cars AS c ON c.id = cp.car_id
    LEFT JOIN geofences AS geo ON geo.id = cp.geofence_id
    LEFT JOIN addresses AS addr ON addr.id = cp.address_id
"""


@mcp.tool(
    description=(
        "查询最近的充电记录列表。"
        "返回充电时间、地点、电量、费用等信息。"
        "当用户想查看充电历史，或需要找到某条充电记录 ID 时使用。"
    )
)
async def list_recent_charges(
    ctx: Context,
    limit: int = DEFAULT_LIMIT,
    only_missing_cost: bool = False,
) -> dict[str, Any]:
    app = _app_context(ctx)
    normalized_limit = _normalize_limit(limit)

    cost_filter = "WHERE cp.cost IS NULL" if only_missing_cost else ""

    query = (
        BASE_CHARGE_SELECT
        + f"""
        {cost_filter}
        ORDER BY cp.start_date DESC NULLS LAST, cp.id DESC
        LIMIT $1
        """
    )

    async with app.pool.acquire() as conn:
        rows = await conn.fetch(query, normalized_limit)

    return {
        "count": len(rows),
        "limit": normalized_limit,
        "only_missing_cost": only_missing_cost,
        "timezone": app.local_timezone.key,
        "charges": [
            _charge_payload(row, app.local_timezone)
            for row in rows
        ],
    }


@mcp.tool(
    description=(
        "查看某次充电的详细信息，包括电量、时长、电池变化、"
        "续航增加、地点、费用等。填写费用前可用来确认是哪次充电。"
    )
)
async def get_charge_detail(
    ctx: Context,
    charge_id: int,
) -> dict[str, Any]:
    app = _app_context(ctx)

    query = BASE_CHARGE_SELECT + "WHERE cp.id = $1"

    async with app.pool.acquire() as conn:
        row = await conn.fetchrow(query, charge_id)

    if row is None:
        return {
            "found": False,
            "charge_id": charge_id,
        }

    return {
        "found": True,
        "timezone": app.local_timezone.key,
        "charge": _charge_payload(row, app.local_timezone),
    }


@mcp.tool(
    description=(
        "为某次充电记录填写或更新费用金额。"
        "这是核心功能，用于手动记录公共充电站的实际花费。"
        "费用写入 TeslaMate 的 charging_processes.cost 字段。"
    )
)
async def set_charge_cost(
    ctx: Context,
    charge_id: int,
    cost: int | float | str,
    currency: str | None = None,
) -> dict[str, Any]:
    app = _app_context(ctx)
    normalized_cost = _normalize_cost(cost)

    query = BASE_CHARGE_SELECT + "WHERE cp.id = $1"

    async with app.pool.acquire() as conn:
        async with conn.transaction():
            row_before = await conn.fetchrow(query, charge_id)

            if row_before is None:
                return {
                    "updated": False,
                    "charge_id": charge_id,
                    "reason": "not_found",
                }

            await conn.execute(
                """
                UPDATE charging_processes
                SET cost = $2
                WHERE id = $1
                """,
                charge_id,
                normalized_cost,
            )

            row_after = await conn.fetchrow(query, charge_id)

    return {
        "updated": True,
        "charge_id": charge_id,
        "previous_cost": _format_decimal(row_before["cost"]),
        "new_cost": _format_decimal(normalized_cost),
        "currency": currency,
        "currency_persisted": False,
        "currency_note": (
            "TeslaMate only stores charging_processes.cost; "
            "currency is returned as text and is not persisted."
        ),
        "charge": _charge_payload(row_after, app.local_timezone),
    }


@mcp.tool(
    description=(
        "按日期范围查询充电记录。"
        "适合「昨天的充电」「上周的充电」「本月充电记录」等场景。"
        "日期按 LOCAL_TIMEZONE 本地时区解释。"
    )
)
async def search_charges_by_date(
    ctx: Context,
    start_date: str,
    end_date: str,
    limit: int = 50,
    only_missing_cost: bool = False,
) -> dict[str, Any]:
    app = _app_context(ctx)

    normalized_limit = _normalize_limit(limit, default=50)

    start_boundary = _parse_date_boundary(
        start_date,
        local_timezone=app.local_timezone,
        end_boundary=False,
    )

    end_boundary = _parse_date_boundary(
        end_date,
        local_timezone=app.local_timezone,
        end_boundary=True,
    )

    if end_boundary <= start_boundary:
        raise ValueError("end_date must be after start_date")

    cost_filter = "AND cp.cost IS NULL" if only_missing_cost else ""

    query = (
        BASE_CHARGE_SELECT
        + f"""
        WHERE cp.start_date >= $1
          AND cp.start_date < $2
          {cost_filter}
        ORDER BY cp.start_date DESC NULLS LAST, cp.id DESC
        LIMIT $3
        """
    )

    async with app.pool.acquire() as conn:
        rows = await conn.fetch(
            query,
            start_boundary,
            end_boundary,
            normalized_limit,
        )

    return {
        "count": len(rows),
        "limit": normalized_limit,
        "only_missing_cost": only_missing_cost,
        "timezone": app.local_timezone.key,
        "start_date": start_date,
        "end_date": end_date,
        "start_boundary_utc": start_boundary.isoformat() + "Z",
        "end_boundary_utc_exclusive": end_boundary.isoformat() + "Z",
        "charges": [
            _charge_payload(row, app.local_timezone)
            for row in rows
        ],
    }


@mcp.tool(
    description=(
        "统计指定时间段内的充电费用汇总，"
        "包括充电场次、已填写费用场次、未填写费用场次、总费用、总电量、平均每度电费用。"
    )
)
async def get_cost_summary(
    ctx: Context,
    start_date: str,
    end_date: str,
) -> dict[str, Any]:
    app = _app_context(ctx)

    start_boundary = _parse_date_boundary(
        start_date,
        local_timezone=app.local_timezone,
        end_boundary=False,
    )

    end_boundary = _parse_date_boundary(
        end_date,
        local_timezone=app.local_timezone,
        end_boundary=True,
    )

    if end_boundary <= start_boundary:
        raise ValueError("end_date must be after start_date")

    query = """
        SELECT
            COUNT(*) AS total_sessions,
            COUNT(cost) AS sessions_with_cost,
            COALESCE(SUM(cost), 0) AS total_cost,
            COALESCE(SUM(charge_energy_added), 0) AS total_energy_kwh
        FROM charging_processes
        WHERE end_date IS NOT NULL
          AND start_date >= $1
          AND start_date < $2
    """

    async with app.pool.acquire() as conn:
        row = await conn.fetchrow(query, start_boundary, end_boundary)

    total_sessions = int(row["total_sessions"])
    sessions_with_cost = int(row["sessions_with_cost"])
    total_cost = Decimal(str(row["total_cost"])).quantize(Decimal("0.01"))
    total_energy = Decimal(str(row["total_energy_kwh"]))

    missing_sessions = total_sessions - sessions_with_cost

    if total_energy > 0:
        avg_cost_per_kwh = (total_cost / total_energy).quantize(Decimal("0.001"))
    else:
        avg_cost_per_kwh = Decimal("0.000")

    return {
        "timezone": app.local_timezone.key,
        "start_date": start_date,
        "end_date": end_date,
        "start_boundary_utc": start_boundary.isoformat() + "Z",
        "end_boundary_utc_exclusive": end_boundary.isoformat() + "Z",
        "total_sessions": total_sessions,
        "sessions_with_cost": sessions_with_cost,
        "missing_cost_sessions": missing_sessions,
        "total_cost": str(total_cost),
        "total_energy_kwh": str(total_energy.quantize(Decimal("0.1"))),
        "avg_cost_per_kwh": str(avg_cost_per_kwh),
    }


def _transport() -> str:
    transport = os.getenv("MCP_TRANSPORT", "streamable-http").strip().lower()

    if transport not in SUPPORTED_TRANSPORTS:
        supported = ", ".join(sorted(SUPPORTED_TRANSPORTS))
        raise RuntimeError(f"MCP_TRANSPORT must be one of: {supported}")

    return transport


if __name__ == "__main__":
    mcp.run(transport=_transport())