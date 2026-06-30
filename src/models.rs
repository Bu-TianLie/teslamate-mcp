use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormattedDatetime {
    pub utc: Option<String>,
    pub local: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChargeRecord {
    pub id: i32,
    pub car_id: i16,
    pub car_name: Option<String>,
    pub start_date: FormattedDatetime,
    pub end_date: FormattedDatetime,
    pub duration_min: Option<i16>,
    pub location: String,
    pub geofence_id: Option<i32>,
    pub geofence_name: Option<String>,
    pub address_id: Option<i32>,
    pub address: Option<String>,
    pub position_id: Option<i32>,
    pub start_battery_level: Option<i16>,
    pub end_battery_level: Option<i16>,
    pub start_ideal_range_km: Option<f64>,
    pub end_ideal_range_km: Option<f64>,
    pub range_gained_km: Option<f64>,
    pub charge_energy_added_kwh: Option<f64>,
    pub charge_energy_used_kwh: Option<f64>,
    pub cost: Option<String>,
    pub has_cost: bool,
}

#[derive(Debug, Serialize)]
pub struct ListChargesResponse {
    pub count: usize,
    pub limit: i32,
    pub only_missing_cost: bool,
    pub timezone: String,
    pub charges: Vec<ChargeRecord>,
}

#[derive(Debug, Serialize)]
pub struct ChargeDetailResponse {
    pub found: bool,
    pub charge_id: i32,
    pub timezone: Option<String>,
    pub charge: Option<ChargeRecord>,
}

#[derive(Debug, Serialize)]
pub struct SetCostResponse {
    pub updated: bool,
    pub charge_id: i32,
    pub previous_cost: Option<String>,
    pub new_cost: Option<String>,
    pub currency: Option<String>,
    pub currency_persisted: bool,
    pub currency_note: String,
    pub charge: Option<ChargeRecord>,
}

#[derive(Debug, Serialize)]
pub struct SearchChargesResponse {
    pub count: usize,
    pub limit: i32,
    pub only_missing_cost: bool,
    pub timezone: String,
    pub start_date: String,
    pub end_date: String,
    pub start_boundary_utc: String,
    pub end_boundary_utc_exclusive: String,
    pub charges: Vec<ChargeRecord>,
}

#[derive(Debug, Serialize)]
pub struct CostSummaryResponse {
    pub timezone: String,
    pub start_date: String,
    pub end_date: String,
    pub start_boundary_utc: String,
    pub end_boundary_utc_exclusive: String,
    pub total_sessions: i64,
    pub sessions_with_cost: i64,
    pub missing_cost_sessions: i64,
    pub total_cost: String,
    pub total_energy_kwh: String,
    pub avg_cost_per_kwh: String,
}
