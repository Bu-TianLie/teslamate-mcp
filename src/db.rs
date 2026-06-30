use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use rust_decimal::Decimal;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

use crate::error::AppError;
use crate::models::{ChargeRecord, FormattedDatetime};

const MAX_LIMIT: i32 = 100;
pub const DEFAULT_LIMIT: i32 = 10;

const BASE_CHARGE_SELECT: &str = r#"
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
"#;

#[derive(Debug)]
pub struct Database {
    pub pool: PgPool,
    pub local_timezone: Tz,
}

impl Database {
    pub async fn new(database_url: &str, local_timezone: Tz) -> Result<Self, AppError> {
        let min_size = std::env::var("DATABASE_POOL_MIN_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        let max_size = std::env::var("DATABASE_POOL_MAX_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);
        let command_timeout: f64 = std::env::var("DATABASE_COMMAND_TIMEOUT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10.0);

        let pool = PgPoolOptions::new()
            .min_connections(min_size)
            .max_connections(max_size)
            .acquire_timeout(std::time::Duration::from_secs_f64(command_timeout))
            .connect(database_url)
            .await?;

        Ok(Self {
            pool,
            local_timezone,
        })
    }

    pub fn normalize_limit(&self, limit: Option<i32>, default: i32) -> i32 {
        let limit = limit.unwrap_or(default);
        let limit = limit.max(1);
        limit.min(MAX_LIMIT)
    }

    pub fn normalize_cost(&self, cost: serde_json::Value) -> Result<Decimal, AppError> {
        let cost_str = match cost {
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s,
            _ => return Err(AppError::Validation("cost must be a number or string".into())),
        };

        let decimal: Decimal = cost_str
            .parse()
            .map_err(|_| AppError::Validation("cost must be a valid number".into()))?;

        if decimal < Decimal::ZERO {
            return Err(AppError::Validation("cost must be >= 0".into()));
        }

        let max_cost: Decimal = "999999.99".parse().unwrap();
        if decimal > max_cost {
            return Err(AppError::Validation("cost must be <= 999999.99".into()));
        }

        Ok(decimal.round_dp(2))
    }

    pub fn parse_date_boundary(
        &self,
        raw: &str,
        end_boundary: bool,
    ) -> Result<DateTime<Utc>, AppError> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(AppError::Validation("date cannot be empty".into()));
        }

        // Try parsing as date-only (YYYY-MM-DD)
        if let Ok(date) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
            let date = if end_boundary {
                date + chrono::Duration::days(1)
            } else {
                date
            };
            let local_dt = self
                .local_timezone
                .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
                .single()
                .ok_or_else(|| AppError::Validation("ambiguous local date".into()))?;
            return Ok(local_dt.with_timezone(&Utc));
        }

        // Try parsing as ISO datetime
        let normalized = if raw.ends_with('Z') || raw.ends_with('z') {
            format!("{}+00:00", &raw[..raw.len() - 1])
        } else {
            raw.to_string()
        };
        let dt = DateTime::parse_from_rfc3339(&normalized)
            .or_else(|_| {
                // Try without timezone — use NaiveDateTime, not NaiveDate
                let ndt = NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S")
                    .map_err(|_| chrono::format::ParseErrorKind::Impossible)?;
                self.local_timezone
                    .from_local_datetime(&ndt)
                    .single()
                    .ok_or(chrono::format::ParseErrorKind::Impossible)
                    .map(|dt| dt.fixed_offset())
            })
            .map_err(|_| {
                AppError::Validation(
                    "dates must use ISO format, e.g. 2026-05-03 or 2026-05-03T10:30:00".into(),
                )
            })?;

        Ok(dt.with_timezone(&Utc))
    }

    fn format_datetime(&self, value: Option<NaiveDateTime>) -> FormattedDatetime {
        match value {
            Some(ndt) => {
                // TeslaMate stores UTC timestamps as naive TIMESTAMP
                let utc_dt = Utc.from_utc_datetime(&ndt);
                let utc = utc_dt.to_rfc3339().replace("+00:00", "Z");
                let local = utc_dt.with_timezone(&self.local_timezone).to_rfc3339();
                FormattedDatetime {
                    utc: Some(utc),
                    local: Some(local),
                }
            }
            None => FormattedDatetime {
                utc: None,
                local: None,
            },
        }
    }

    fn row_to_charge_record(&self, row: &sqlx::postgres::PgRow) -> ChargeRecord {
        let geofence_name: Option<String> = row.get("geofence_name");
        let address: Option<String> = row.get("address");
        let location = geofence_name
            .clone()
            .or(address.clone())
            .unwrap_or_else(|| "未知地点".to_string());

        let start_ideal_range_km: Option<Decimal> = row.get("start_ideal_range_km");
        let end_ideal_range_km: Option<Decimal> = row.get("end_ideal_range_km");
        let range_gained_km = match (start_ideal_range_km, end_ideal_range_km) {
            (Some(start), Some(end)) => Some((end - start).to_string().parse::<f64>().unwrap_or(0.0)),
            _ => None,
        };

        let cost: Option<Decimal> = row.get("cost");

        ChargeRecord {
            id: row.get("id"),
            car_id: row.get("car_id"),
            car_name: row.get("car_name"),
            start_date: self.format_datetime(row.get("start_date")),
            end_date: self.format_datetime(row.get("end_date")),
            duration_min: row.get("duration_min"),
            location,
            geofence_id: row.get("geofence_id"),
            geofence_name,
            address_id: row.get("address_id"),
            address,
            position_id: row.get("position_id"),
            start_battery_level: row.get("start_battery_level"),
            end_battery_level: row.get("end_battery_level"),
            start_ideal_range_km: start_ideal_range_km.map(|v| v.to_string().parse().unwrap_or(0.0)),
            end_ideal_range_km: end_ideal_range_km.map(|v| v.to_string().parse().unwrap_or(0.0)),
            range_gained_km,
            charge_energy_added_kwh: row
                .get::<Option<Decimal>, _>("charge_energy_added")
                .map(|v| v.to_string().parse().unwrap_or(0.0)),
            charge_energy_used_kwh: row
                .get::<Option<Decimal>, _>("charge_energy_used")
                .map(|v| v.to_string().parse().unwrap_or(0.0)),
            cost: cost.map(|v| v.round_dp(2).to_string()),
            has_cost: cost.is_some(),
        }
    }

    pub async fn list_recent_charges(
        &self,
        limit: Option<i32>,
        only_missing_cost: bool,
    ) -> Result<Vec<ChargeRecord>, AppError> {
        let limit = self.normalize_limit(limit, DEFAULT_LIMIT);
        let cost_filter = if only_missing_cost {
            "WHERE cp.cost IS NULL"
        } else {
            ""
        };

        let query = format!(
            "{BASE_CHARGE_SELECT} {cost_filter} ORDER BY cp.start_date DESC NULLS LAST, cp.id DESC LIMIT $1"
        );

        let rows = sqlx::query(&query)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.iter().map(|r| self.row_to_charge_record(r)).collect())
    }

    pub async fn get_charge_detail(
        &self,
        charge_id: i32,
    ) -> Result<Option<ChargeRecord>, AppError> {
        let query = format!("{BASE_CHARGE_SELECT} WHERE cp.id = $1");

        let row = sqlx::query(&query)
            .bind(charge_id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(|r| self.row_to_charge_record(&r)))
    }

    pub async fn set_charge_cost(
        &self,
        charge_id: i32,
        cost: Decimal,
    ) -> Result<Option<(Option<Decimal>, ChargeRecord)>, AppError> {
        let mut tx = self.pool.begin().await?;

        let query = format!("{BASE_CHARGE_SELECT} WHERE cp.id = $1");

        let row = sqlx::query(&query)
            .bind(charge_id)
            .fetch_optional(&mut *tx)
            .await?;

        let row = match row {
            Some(r) => r,
            None => {
                tx.rollback().await?;
                return Ok(None);
            }
        };

        let previous_cost: Option<Decimal> = row.get("cost");

        // Update cost
        sqlx::query("UPDATE charging_processes SET cost = $2 WHERE id = $1")
            .bind(charge_id)
            .bind(cost)
            .execute(&mut *tx)
            .await?;

        // Fetch updated record
        let updated_row = sqlx::query(&query)
            .bind(charge_id)
            .fetch_one(&mut *tx)
            .await?;

        tx.commit().await?;

        Ok(Some((previous_cost, self.row_to_charge_record(&updated_row))))
    }

    pub async fn search_charges_by_date(
        &self,
        start_date: &str,
        end_date: &str,
        limit: Option<i32>,
        only_missing_cost: bool,
    ) -> Result<(DateTime<Utc>, DateTime<Utc>, Vec<ChargeRecord>), AppError> {
        let limit = self.normalize_limit(limit, 50);
        let start_boundary = self.parse_date_boundary(start_date, false)?;
        let end_boundary = self.parse_date_boundary(end_date, true)?;

        if end_boundary <= start_boundary {
            return Err(AppError::Validation("end_date must be after start_date".into()));
        }

        let cost_filter = if only_missing_cost {
            "AND cp.cost IS NULL"
        } else {
            ""
        };

        let query = format!(
            "{BASE_CHARGE_SELECT} WHERE cp.start_date >= $1 AND cp.start_date < $2 {cost_filter} ORDER BY cp.start_date DESC NULLS LAST, cp.id DESC LIMIT $3"
        );

        let rows = sqlx::query(&query)
            .bind(start_boundary)
            .bind(end_boundary)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;

        Ok((
            start_boundary,
            end_boundary,
            rows.iter().map(|r| self.row_to_charge_record(r)).collect(),
        ))
    }

    pub async fn get_cost_summary(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<(DateTime<Utc>, DateTime<Utc>, i64, i64, Decimal, Decimal), AppError> {
        let start_boundary = self.parse_date_boundary(start_date, false)?;
        let end_boundary = self.parse_date_boundary(end_date, true)?;

        if end_boundary <= start_boundary {
            return Err(AppError::Validation("end_date must be after start_date".into()));
        }

        let query = r#"
            SELECT
                COUNT(*) AS total_sessions,
                COUNT(cost) AS sessions_with_cost,
                COALESCE(SUM(cost), 0) AS total_cost,
                COALESCE(SUM(charge_energy_added), 0) AS total_energy_kwh
            FROM charging_processes
            WHERE end_date IS NOT NULL
              AND start_date >= $1
              AND start_date < $2
        "#;

        let row = sqlx::query(&query)
            .bind(start_boundary)
            .bind(end_boundary)
            .fetch_one(&self.pool)
            .await?;

        let total_sessions: i64 = row.get("total_sessions");
        let sessions_with_cost: i64 = row.get("sessions_with_cost");
        let total_cost: Decimal = row.get("total_cost");
        let total_energy: Decimal = row.get("total_energy_kwh");

        Ok((
            start_boundary,
            end_boundary,
            total_sessions,
            sessions_with_cost,
            total_cost.round_dp(2),
            total_energy,
        ))
    }
}
