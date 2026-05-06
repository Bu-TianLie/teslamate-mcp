FROM python:3.11-slim-bookworm

ARG PYPI_INDEX_URL=https://mirrors.aliyun.com/pypi/simple/
ARG UV_VERSION=0.11.7

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    PIP_INDEX_URL=${PYPI_INDEX_URL} \
    UV_COMPILE_BYTECODE=1 \
    UV_INDEX_URL=${PYPI_INDEX_URL} \
    UV_LINK_MODE=copy \
    UV_PYTHON_DOWNLOADS=0 \
    UV_PROJECT_ENVIRONMENT=/opt/venv

WORKDIR /app

RUN python -m pip install --no-cache-dir "uv==${UV_VERSION}"

COPY .python-version pyproject.toml uv.lock uv.toml README.md ./
COPY src ./src
RUN uv sync --no-dev --frozen

EXPOSE 8000

CMD ["/opt/venv/bin/python", "src/server.py"]
