use std::net::SocketAddr;
use std::sync::Arc;

use rmcp::{
    ServerHandler,
    model::{ServerCapabilities, ServerInfo},
    tool,
};
use serde_json::json;

use crate::db::Database;
use crate::models::*;

/// Format a UTC DateTime as ISO 8601 with 'Z' suffix
fn utc_to_z(dt: &chrono::DateTime<chrono::Utc>) -> String {
    dt.to_rfc3339().replace("+00:00", "Z")
}

#[derive(Debug, Clone)]
pub struct TeslaMateServer {
    db: Arc<Database>,
}

impl TeslaMateServer {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

#[tool(tool_box)]
impl TeslaMateServer {
    #[tool(
        name = "list_recent_charges",
        description = "查询最近的充电记录列表。返回充电时间、地点、电量、费用等信息。当用户想查看充电历史，或需要找到某条充电记录 ID 时使用。"
    )]
    async fn list_recent_charges(
        &self,
        #[tool(param)]
        #[schemars(description = "返回记录数量，默认10，最大100")]
        limit: Option<i32>,
        #[tool(param)]
        #[schemars(description = "是否只返回未填写费用的记录")]
        only_missing_cost: Option<bool>,
    ) -> String {
        let only_missing_cost = only_missing_cost.unwrap_or(false);
        match self.db.list_recent_charges(limit, only_missing_cost).await {
            Ok(charges) => {
                let response = ListChargesResponse {
                    count: charges.len(),
                    limit: self.db.normalize_limit(limit, crate::db::DEFAULT_LIMIT),
                    only_missing_cost,
                    timezone: self.db.local_timezone.to_string(),
                    charges,
                };
                serde_json::to_string(&response).unwrap_or_else(|e| {
                    json!({"error": e.to_string()}).to_string()
                })
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
        #[tool(param)]
        #[schemars(description = "充电记录 ID")]
        charge_id: i32,
    ) -> String {
        match self.db.get_charge_detail(charge_id).await {
            Ok(Some(charge)) => {
                let response = ChargeDetailResponse {
                    found: true,
                    charge_id,
                    timezone: Some(self.db.local_timezone.to_string()),
                    charge: Some(charge),
                };
                serde_json::to_string(&response).unwrap_or_else(|e| {
                    json!({"error": e.to_string()}).to_string()
                })
            }
            Ok(None) => {
                let response = ChargeDetailResponse {
                    found: false,
                    charge_id,
                    timezone: None,
                    charge: None,
                };
                serde_json::to_string(&response).unwrap_or_else(|e| {
                    json!({"error": e.to_string()}).to_string()
                })
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
        #[tool(param)]
        #[schemars(description = "充电记录 ID")]
        charge_id: i32,
        #[tool(param)]
        #[schemars(description = "费用金额")]
        cost: serde_json::Value,
        #[tool(param)]
        #[schemars(description = "货币类型（仅用于返回说明，不存储）")]
        currency: Option<String>,
    ) -> String {
        let normalized_cost = match self.db.normalize_cost(cost) {
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
                serde_json::to_string(&response).unwrap_or_else(|e| {
                    json!({"error": e.to_string()}).to_string()
                })
            }
            Ok(None) => {
                json!({
                    "updated": false,
                    "charge_id": charge_id,
                    "reason": "not_found"
                })
                .to_string()
            }
            Err(e) => json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(
        name = "search_charges_by_date",
        description = "按日期范围查询充电记录。适合「昨天的充电」「上周的充电」「本月充电记录」等场景。日期按 LOCAL_TIMEZONE 本地时区解释。"
    )]
    async fn search_charges_by_date(
        &self,
        #[tool(param)]
        #[schemars(description = "开始日期，格式 YYYY-MM-DD")]
        start_date: String,
        #[tool(param)]
        #[schemars(description = "结束日期，格式 YYYY-MM-DD")]
        end_date: String,
        #[tool(param)]
        #[schemars(description = "返回记录数量，默认50，最大100")]
        limit: Option<i32>,
        #[tool(param)]
        #[schemars(description = "是否只返回未填写费用的记录")]
        only_missing_cost: Option<bool>,
    ) -> String {
        let only_missing_cost = only_missing_cost.unwrap_or(false);
        match self
            .db
            .search_charges_by_date(&start_date, &end_date, limit, only_missing_cost)
            .await
        {
            Ok((start_boundary, end_boundary, charges)) => {
                let response = SearchChargesResponse {
                    count: charges.len(),
                    limit: self.db.normalize_limit(limit, 50),
                    only_missing_cost,
                    timezone: self.db.local_timezone.to_string(),
                    start_date,
                    end_date,
                    start_boundary_utc: utc_to_z(&start_boundary),
                    end_boundary_utc_exclusive: utc_to_z(&end_boundary),
                    charges,
                };
                serde_json::to_string(&response).unwrap_or_else(|e| {
                    json!({"error": e.to_string()}).to_string()
                })
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
        #[tool(param)]
        #[schemars(description = "开始日期，格式 YYYY-MM-DD")]
        start_date: String,
        #[tool(param)]
        #[schemars(description = "结束日期，格式 YYYY-MM-DD")]
        end_date: String,
    ) -> String {
        match self.db.get_cost_summary(&start_date, &end_date).await {
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
                    start_date,
                    end_date,
                    start_boundary_utc: utc_to_z(&start_boundary),
                    end_boundary_utc_exclusive: utc_to_z(&end_boundary),
                    total_sessions,
                    sessions_with_cost,
                    missing_cost_sessions: missing_sessions,
                    total_cost: total_cost.to_string(),
                    total_energy_kwh: total_energy.round_dp(1).to_string(),
                    avg_cost_per_kwh: avg_cost_per_kwh.to_string(),
                };
                serde_json::to_string(&response).unwrap_or_else(|e| {
                    json!({"error": e.to_string()}).to_string()
                })
            }
            Err(e) => json!({"error": e.to_string()}).to_string(),
        }
    }
}

#[tool(tool_box)]
impl ServerHandler for TeslaMateServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "TeslaMate 充电费用助手。可以查询 TeslaMate 充电记录，查看充电详情，\
                 筛选未填写费用的记录，并手动更新公共充电站实际费用。\
                 费用写入 charging_processes.cost。\
                 TeslaMate 本身不存储货币类型，currency 只作为返回说明。"
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

pub async fn serve_stdio(db: Arc<Database>) -> Result<(), anyhow::Error> {
    use rmcp::ServiceExt;
    let server = TeslaMateServer::new(db);
    let transport = rmcp::transport::stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;
    Ok(())
}

pub async fn serve_sse(db: Arc<Database>, host: &str, port: u16) -> Result<(), anyhow::Error> {
    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    tracing::info!("SSE transport listening on {addr}");
    let sse_server = rmcp::transport::SseServer::serve(addr).await?;
    let ct = sse_server.with_service(move || TeslaMateServer::new(db.clone()));
    ct.cancelled().await;
    Ok(())
}
