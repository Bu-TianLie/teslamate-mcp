# TeslaMate Charging Cost MCP Server

一个 MCP 服务，用来查询 TeslaMate 最近充电记录，并直接写入 `charging_processes.cost`。

默认使用 `streamable-http`，适合小米手机 MiClaw 这类通过网络 URL 调用 MCP Server 的客户端：

```text
http://<TeslaMate 所在机器的局域网 IP>:8000/mcp
```

也可以通过 `MCP_TRANSPORT=stdio` 切回 Claude Desktop / Claude Code 常见的 stdio 模式。

如果 MiClaw 只支持旧的 SSE transport，可以设置 `MCP_TRANSPORT=sse`，默认地址会是：

```text
http://<TeslaMate 所在机器的局域网 IP>:8000/sse
```

## Tools

- `list_recent_charges(limit=10)`：查询最近充电记录。
- `get_charge_detail(charge_id)`：查看单次充电详情。
- `set_charge_cost(charge_id, cost, currency=None)`：写入充电费用。TeslaMate 只保存金额，`currency` 仅用于返回确认信息。
- `search_charges_by_date(start_date, end_date, limit=50)`：按日期范围查询。`end_date` 如果是 `YYYY-MM-DD`，会包含该日整天。

## Docker 部署

项目已经包含 `docker-compose.yml`，适合把这个 MCP 服务作为独立容器部署，并接入现有 TeslaMate 的 Docker 网络。

1. 复制环境变量：

```bash
cp .env.example .env
```

2. 找到 TeslaMate 所在 Docker 网络：

```bash
docker network ls | grep teslamate
```

3. 编辑 `.env`：

```dotenv
DATABASE_URL=postgresql://teslamate:你的数据库密码@database:5432/teslamate
TESLAMATE_DOCKER_NETWORK=teslamate_default
MCP_TRANSPORT=streamable-http
MCP_HOST=0.0.0.0
MCP_PORT=8000
LOCAL_TIMEZONE=Asia/Shanghai
```

如果数据库密码里有 `@`、`#`、`:`、`/` 等特殊字符，需要 URL encode。例如 `p@ss` 要写成 `p%40ss`。

4. 构建并启动：

```bash
docker compose up -d --build
```

Dockerfile 默认使用 Docker Hub 官方 `python:3.11-slim-bookworm`，并通过阿里云 PyPI 镜像安装 `uv`，避免拉取较慢的 `ghcr.io/astral-sh/uv`。如果你想换 PyPI 镜像，可以在构建时传参：

```bash
docker compose build --build-arg PYPI_INDEX_URL=https://pypi.tuna.tsinghua.edu.cn/simple/
docker compose up -d
```

如果 Docker Hub 官方 Python 镜像也慢，建议在服务器 Docker daemon 配置阿里云 ACR 镜像加速器，或把 `python:3.11-slim-bookworm` 同步到你自己的阿里云 ACR 私有仓库后再改 `FROM`。

5. 查看日志：

```bash
docker compose logs -f teslamate-mcp
```

然后在小米 MiClaw 里配置 MCP Server URL：

```text
http://<TeslaMate 所在机器的局域网 IP>:8000/mcp
```

例如 TeslaMate 主机局域网 IP 是 `192.168.31.20`：

```text
http://192.168.31.20:8000/mcp
```

手机和 TeslaMate 主机需要在同一个局域网，或者手机需要能通过 VPN / 内网穿透访问这个地址。

如果你想把服务直接合并到现有 TeslaMate 的 `docker-compose.yml`，可以把本项目 `docker-compose.yml` 里的 `teslamate-mcp` service 复制过去，把 `build.context` 改成 `./teslamate-mcp`，并删除 `networks` 里的 `external: true` 配置。

## 安全提醒

这个服务可以写入 TeslaMate 数据库的充电费用，不建议直接暴露到公网。如果必须远程访问，建议放在 VPN、Tailscale、ZeroTier、Cloudflare Tunnel Access 或带鉴权的反向代理后面。

## Local Run

```bash
cp .env.example .env
uv python install 3.11
uv sync --no-dev
uv run python src/server.py
```

默认会启动 HTTP MCP。stdio 模式可以这样运行：

```bash
MCP_TRANSPORT=stdio uv run python src/server.py
```

SSE 模式可以这样运行：

```bash
MCP_TRANSPORT=sse uv run python src/server.py
```

## Claude Desktop stdio 配置示例

如果 Claude Desktop 在宿主机直接启动 Docker 容器，可以把 transport 改成 stdio：

```json
{
  "mcpServers": {
    "teslamate": {
      "command": "docker",
      "args": [
        "compose",
        "run",
        "--rm",
        "-i",
        "-e",
        "MCP_TRANSPORT=stdio",
        "teslamate-mcp"
      ]
    }
  }
}
```

如果你已经把服务作为长期容器运行，也可以改成 `docker exec -i <container> python src/server.py` 一类的命令。
