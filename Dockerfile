FROM ghcr.io/astral-sh/uv:python3.11-bookworm-slim

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    UV_COMPILE_BYTECODE=1 \
    UV_LINK_MODE=copy \
    UV_PROJECT_ENVIRONMENT=/opt/venv

WORKDIR /app

COPY .python-version pyproject.toml uv.lock uv.toml README.md ./
COPY src ./src
RUN uv sync --no-dev --frozen

EXPOSE 8000

CMD ["/opt/venv/bin/python", "src/server.py"]
