#!/bin/bash
# test_mcp.sh - 测试 TeslaMate MCP Server

MCP_URL="http://localhost:8000/mcp"

echo "=== 1. Initialize ==="
curl -s -X POST "$MCP_URL" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  --data-raw '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}'

echo -e "\n\n=== 2. List Tools ==="
curl -s -X POST "$MCP_URL" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  --data-raw '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'

echo -e "\n\n=== 3. Call list_recent_charges ==="
curl -s -X POST "$MCP_URL" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  --data-raw '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_recent_charges","arguments":{"limit":3}}}'

echo -e "\n\n=== 4. Call get_cost_summary ==="
curl -s -X POST "$MCP_URL" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  --data-raw '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"get_cost_summary","arguments":{"start_date":"2026-01-01","end_date":"2026-06-30"}}}'
