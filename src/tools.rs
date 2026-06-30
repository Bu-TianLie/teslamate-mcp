use std::net::SocketAddr;
use std::sync::Arc;

use rmcp::{
    ServerHandler, ServiceExt, handler::server::{router::tool::ToolRouter, wrapper::Parameters}, schemars, tool, tool_handler, tool_router, transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    }
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::db::Database;
use crate::models::*;

/// Format a UTC DateTime as ISO 8601 with 'Z' suffix
fn utc_to_z(dt: &chrono::DateTime<chrono::Utc>) -> String {
    dt.to_rfc3339().replace("+00:00", "Z")
}

// ─── Tool parameter schemas ───

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ListRecentChargesParams {
    /// 返回记录数量，默认10，最大100
    #[schemars(description = "返回记录数量，默认10，最大100")]
    pub limit: Option<i32>,
    /// 是否只返回未填写费用的记录
    #[schemars(description = "是否只返回未填写费用的记录")]
    pub only_missing_cost: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GetChargeDetailParams {
    /// 充电记录 ID
    #[schemars(description = "充电记录 ID")]
    pub charge_id: i32,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SetChargeCostParams {
    /// 充电记录 ID
    #[schemars(description = "充电记录 ID")]
    pub charge_id: i32,
    /// 费用金额
    #[schemars(description = "费用金额")]
    pub cost: serde_json::Value,
    /// 货币类型（仅用于返回说明，不存储）
    #[schemars(description = "货币类型（仅用于返回说明，不存储）")]
    pub currency: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SearchChargesByDateParams {
    /// 开始日期，格式 YYYY-MM-DD
    #[schemars(description = "开始日期，格式 YYYY-MM-DD")]
    pub start_date: String,
    /// 结束日期，格式 YYYY-MM-DD
    #[schemars(description = "结束日期，格式 YYYY-MM-DD")]
    pub end_date: String,
    /// 返回记录数量，默认50，最大100
    #[schemars(description = "返回记录数量，默认50，最大100")]
    pub limit: Option<i32>,
    /// 是否只返回未填写费用的记录
    #[schemars(description = "是否只返回未填写费用的记录")]
    pub only_missing_cost: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GetCostSummaryParams {
    /// 开始日期，格式 YYYY-MM-DD
    #[schemars(description = "开始日期，格式 YYYY-MM-DD")]
    pub start_date: String,
    /// 结束日期，格式 YYYY-MM-DD
    #[schemars(description = "结束日期，格式 YYYY-MM-DD")]
    pub end_date: String,
}

// ─── Server ───

#[derive(Debug, Clone)]
pub struct TeslaMateServer {
    db: Arc<Database>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl TeslaMateServer {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl TeslaMateServer {
    #[tool(
        name = "list_recent_charges",
        description = "查询最近的充电记录列表。返回充电时间、地点、电量、费用等信息。当用户想查看充电历史，或需要找到某条充电记录 ID 时使用。"
    )]
    async fn list_recent_charges(
        &self,
        Parameters(params): Parameters<ListRecentChargesParams>,
    ) -> String {
        let limit = params.limit;
        let only_missing_cost = params.only_missing_cost.unwrap_or(false);
        match self.db.list_recent_charges(limit, only_missing_cost).await {
            Ok(charges) => {
                let response = ListChargesResponse {
                    count: charges.len(),
                    limit: self.db.normalize_limit(limit, crate::db::DEFAULT_LIMIT),
                    only_missing_cost,
                    timezone: self.db.local_timezone.to_string(),
                    charges,
                };
                serde_json::to_string(&response)
                    .unwrap_or_else(|e| json!({"error": e.to_string()}).to_string())
            }
            Err(e) => json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(
        name = "get_charge_detail",
        description = "查看某次充电的详细信息，包括电量、时长、电池变化、续航增加、地点、费用等。填写费用前可用来确认是哪次充电。"
    )]
    async fn get_charge_detail(
        &self,
        Parameters(params): Parameters<GetChargeDetailParams>,
    ) -> String {
        let charge_id = params.charge_id;
        match self.db.get_charge_detail(charge_id).await {
            Ok(Some(charge)) => {
                let response = ChargeDetailResponse {
                    found: true,
                    charge_id,
                    timezone: Some(self.db.local_timezone.to_string()),
                    charge: Some(charge),
                };
                serde_json::to_string(&response)
                    .unwrap_or_else(|e| json!({"error": e.to_string()}).to_string())
            }
            Ok(None) => {
                let response = ChargeDetailResponse {
                    found: false,
                    charge_id,
                    timezone: None,
                    charge: None,
                };
                serde_json::to_string(&response)
                    .unwrap_or_else(|e| json!({"error": e.to_string()}).to_string())
            }
            Err(e) => json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(
        name = "set_charge_cost",
        description = "为某次充电记录填写或更新费用金额。这是核心功能，用于手动记录公共充电站的实际花费。费用写入 TeslaMate 的 charging_processes.cost 字段。"
    )]
    async fn set_charge_cost(
        &self,
        Parameters(params): Parameters<SetChargeCostParams>,
    ) -> String {
        let charge_id = params.charge_id;
        let currency = params.currency;
        let normalized_cost = match self.db.normalize_cost(params.cost) {
            Ok(c) => c,
            Err(e) => return json!({"error": e.to_string()}).to_string(),
        };

        match self.db.set_charge_cost(charge_id, normalized_cost).await {
            Ok(Some((previous_cost, charge))) => {
                let response = SetCostResponse {
                    updated: true,
                    charge_id,
                    previous_cost: previous_cost.map(|c| c.round_dp(2).to_string()),
                    new_cost: Some(normalized_cost.round_dp(2).to_string()),
                    currency,
                    currency_persisted: false,
                    currency_note: "TeslaMate only stores charging_processes.cost; currency is returned as text and is not persisted.".to_string(),
                    charge: Some(charge),
                };
                serde_json::to_string(&response)
                    .unwrap_or_else(|e| json!({"error": e.to_string()}).to_string())
            }
            Ok(None) => json!({
                "updated": false,
                "charge_id": charge_id,
                "reason": "not_found"
            })
            .to_string(),
            Err(e) => json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(
        name = "search_charges_by_date",
        description = "按日期范围查询充电记录。适合「昨天的充电」「上周的充电」「本月充电记录」等场景。日期按 LOCAL_TIMEZONE 本地时区解释。"
    )]
    async fn search_charges_by_date(
        &self,
        Parameters(params): Parameters<SearchChargesByDateParams>,
    ) -> String {
        let limit = params.limit;
        let only_missing_cost = params.only_missing_cost.unwrap_or(false);
        match self
            .db
            .search_charges_by_date(&params.start_date, &params.end_date, limit, only_missing_cost)
            .await
        {
            Ok((start_boundary, end_boundary, charges)) => {
                let response = SearchChargesResponse {
                    count: charges.len(),
                    limit: self.db.normalize_limit(limit, 50),
                    only_missing_cost,
                    timezone: self.db.local_timezone.to_string(),
                    start_date: params.start_date,
                    end_date: params.end_date,
                    start_boundary_utc: utc_to_z(&start_boundary),
                    end_boundary_utc_exclusive: utc_to_z(&end_boundary),
                    charges,
                };
                serde_json::to_string(&response)
                    .unwrap_or_else(|e| json!({"error": e.to_string()}).to_string())
            }
            Err(e) => json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(
        name = "get_cost_summary",
        description = "统计指定时间段内的充电费用汇总，包括充电场次、已填写费用场次、未填写费用场次、总费用、总电量、平均每度电费用。"
    )]
    async fn get_cost_summary(
        &self,
        Parameters(params): Parameters<GetCostSummaryParams>,
    ) -> String {
        match self
            .db
            .get_cost_summary(&params.start_date, &params.end_date)
            .await
        {
            Ok((
                start_boundary,
                end_boundary,
                total_sessions,
                sessions_with_cost,
                total_cost,
                total_energy,
            )) => {
                let missing_sessions = total_sessions - sessions_with_cost;
                let avg_cost_per_kwh = if total_energy > rust_decimal::Decimal::ZERO {
                    (total_cost / total_energy).round_dp(3)
                } else {
                    rust_decimal::Decimal::ZERO
                };

                let response = CostSummaryResponse {
                    timezone: self.db.local_timezone.to_string(),
                    start_date: params.start_date,
                    end_date: params.end_date,
                    start_boundary_utc: utc_to_z(&start_boundary),
                    end_boundary_utc_exclusive: utc_to_z(&end_boundary),
                    total_sessions,
                    sessions_with_cost,
                    missing_cost_sessions: missing_sessions,
                    total_cost: total_cost.to_string(),
                    total_energy_kwh: total_energy.round_dp(1).to_string(),
                    avg_cost_per_kwh: avg_cost_per_kwh.to_string(),
                };
                serde_json::to_string(&response)
                    .unwrap_or_else(|e| json!({"error": e.to_string()}).to_string())
            }
            Err(e) => json!({"error": e.to_string()}).to_string(),
        }
    }
}

#[tool_handler(
    name = "teslamate-charging-cost-mcp",
    version = "0.1.0",
    instructions = "TeslaMate 充电费用助手。可以查询 TeslaMate 充电记录，查看充电详情，筛选未填写费用的记录，并手动更新公共充电站实际费用。费用写入 charging_processes.cost。TeslaMate 本身不存储货币类型，currency 只作为返回说明。"
)]
impl ServerHandler for TeslaMateServer {}

// ─── Transport: stdio ───

pub async fn serve_stdio(db: Arc<Database>) -> Result<(), anyhow::Error> {
    let server = TeslaMateServer::new(db);
    let transport = rmcp::transport::stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;
    Ok(())
}

// ─── Transport: streamable-http ───

pub async fn serve_http(db: Arc<Database>, host: &str, port: u16) -> Result<(), anyhow::Error> {
    let addr: SocketAddr = format!("{host}:{port}").parse()?;

    let config = StreamableHttpServerConfig::default()
        .with_stateful_mode(false)
        .with_json_response(true);

    let service = StreamableHttpService::new(
        move || Ok(TeslaMateServer::new(db.clone())),
        Arc::new(LocalSessionManager::default()),
        config,
    );

    let router = axum::Router::new().nest_service("/mcp", service);

    tracing::info!("Streamable HTTP transport listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
