use serde::{Deserialize, Serialize};

// ── DB row structs ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OsCase {
    pub id: i64,
    pub os_no: String,
    pub os_date: Option<String>,
    pub os_year: Option<i64>,
    pub location_code: Option<String>,
    pub shift: Option<String>,
    pub booked_by: Option<String>,
    pub pax_name: Option<String>,
    pub pax_nationality: Option<String>,
    pub passport_no: Option<String>,
    pub passport_date: Option<String>,
    pub pp_issue_place: Option<String>,
    pub pax_address1: Option<String>,
    pub pax_address2: Option<String>,
    pub pax_address3: Option<String>,
    pub pax_date_of_birth: Option<String>,
    pub pax_status: Option<String>,
    pub residence_at: Option<String>,
    pub country_of_departure: Option<String>,
    pub port_of_dep_dest: Option<String>,
    pub date_of_departure: Option<String>,
    pub stay_abroad_days: Option<i64>,
    pub flight_no: Option<String>,
    pub flight_date: Option<String>,
    pub total_items: Option<i64>,
    pub total_items_value: Option<f64>,
    pub total_fa_value: Option<f64>,
    pub dutiable_value: Option<f64>,
    pub redeemed_value: Option<f64>,
    pub re_export_value: Option<f64>,
    pub confiscated_value: Option<f64>,
    pub total_duty_amount: Option<f64>,
    pub rf_amount: Option<f64>,
    pub pp_amount: Option<f64>,
    pub ref_amount: Option<f64>,
    pub br_amount: Option<f64>,
    pub wh_amount: Option<f64>,
    pub other_amount: Option<f64>,
    pub total_payable: Option<f64>,
    pub br_no_str: Option<String>,
    pub br_no_num: Option<f64>,
    pub br_date_str: Option<String>,
    pub br_amount_str: Option<String>,
    pub is_legacy: Option<String>,
    pub is_offline_adjudication: Option<String>,
    pub file_spot: Option<String>,
    pub is_draft: Option<String>,
    pub os_printed: Option<String>,
    pub os_category: Option<String>,
    pub online_os: Option<String>,
    pub adjudication_date: Option<String>,
    pub adjudication_time: Option<String>,
    pub adj_offr_name: Option<String>,
    pub adj_offr_designation: Option<String>,
    pub adjn_offr_remarks: Option<String>,
    pub adjn_offr_remarks1: Option<String>,
    pub online_adjn: Option<String>,
    pub unique_no: Option<i64>,
    pub entry_deleted: Option<String>,
    pub bkup_taken: Option<String>,
    pub deleted_by: Option<String>,
    pub deleted_reason: Option<String>,
    pub deleted_on: Option<String>,
    pub detained_by: Option<String>,
    pub seal_no: Option<String>,
    pub nationality: Option<String>,
    pub seizure_date: Option<String>,
    pub dr_no: Option<String>,
    pub dr_year: Option<i64>,
    pub total_drs: Option<i64>,
    pub previous_os_details: Option<String>,
    pub previous_visits: Option<String>,
    pub father_name: Option<String>,
    pub old_passport_no: Option<String>,
    pub total_pkgs: Option<i64>,
    pub supdts_remarks: Option<String>,
    pub supdt_remarks2: Option<String>,
    pub closure_ind: Option<String>,
    pub post_adj_br_entries: Option<String>,
    pub post_adj_dr_no: Option<String>,
    pub post_adj_dr_date: Option<String>,
    pub quashed: Option<String>,
    pub rejected: Option<String>,
    pub pax_name_modified_by_vig: Option<String>,
    // populated from cops_items join
    #[serde(default)]
    pub items: Vec<OsItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OsItem {
    pub id: i64,
    pub os_no: String,
    pub os_year: Option<i64>,
    pub items_sno: i64,
    pub items_desc: Option<String>,
    pub items_qty: Option<f64>,
    pub items_uqc: Option<String>,
    pub value_per_piece: Option<f64>,
    pub items_value: Option<f64>,
    pub items_fa: Option<f64>,
    pub items_fa_type: Option<String>,
    pub items_fa_qty: Option<f64>,
    pub items_fa_uqc: Option<String>,
    pub cumulative_duty_rate: Option<f64>,
    pub items_duty: Option<f64>,
    pub items_duty_type: Option<String>,
    pub items_category: Option<String>,
    pub items_release_category: Option<String>,
    pub items_sub_category: Option<String>,
    pub items_dr_no: Option<i64>,
    pub items_dr_year: Option<i64>,
    pub entry_deleted: Option<String>,
}

// ── Request / Response bodies ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateOsRequest {
    pub os_no: String,
    pub os_date: Option<String>,
    pub os_year: Option<i64>,
    pub location_code: Option<String>,
    pub shift: Option<String>,
    pub booked_by: Option<String>,
    pub pax_name: Option<String>,
    pub pax_nationality: Option<String>,
    pub passport_no: Option<String>,
    pub passport_date: Option<String>,
    pub pp_issue_place: Option<String>,
    pub pax_address1: Option<String>,
    pub pax_address2: Option<String>,
    pub pax_address3: Option<String>,
    pub pax_date_of_birth: Option<String>,
    pub pax_status: Option<String>,
    pub residence_at: Option<String>,
    pub country_of_departure: Option<String>,
    pub port_of_dep_dest: Option<String>,
    pub date_of_departure: Option<String>,
    pub stay_abroad_days: Option<i64>,
    pub flight_no: Option<String>,
    pub flight_date: Option<String>,
    pub detained_by: Option<String>,
    pub seal_no: Option<String>,
    pub seizure_date: Option<String>,
    pub father_name: Option<String>,
    pub old_passport_no: Option<String>,
    pub total_pkgs: Option<i64>,
    pub supdts_remarks: Option<String>,
    pub supdt_remarks2: Option<String>,
    pub previous_os_details: Option<String>,
    pub previous_visits: Option<String>,
    pub case_type: Option<String>,
    pub file_spot: Option<String>,
    pub is_offline_adjudication: Option<String>,
    pub is_draft: Option<String>,
    #[serde(default)]
    pub items: Vec<CreateItemRequest>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CreateItemRequest {
    pub items_sno: i64,
    pub items_desc: Option<String>,
    pub items_qty: Option<f64>,
    pub items_uqc: Option<String>,
    pub value_per_piece: Option<f64>,
    pub items_value: Option<f64>,
    pub items_fa: Option<f64>,
    pub items_fa_type: Option<String>,
    pub items_fa_qty: Option<f64>,
    pub items_fa_uqc: Option<String>,
    pub cumulative_duty_rate: Option<f64>,
    pub items_duty: Option<f64>,
    pub items_duty_type: Option<String>,
    pub items_category: Option<String>,
    pub items_release_category: Option<String>,
    pub items_sub_category: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OsListParams {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub search: Option<String>,
    pub status: Option<String>,    // "pending" | "adjudicated" | "draft"
    pub year: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct OsListResponse {
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
    pub items: Vec<OsCase>,
}

#[derive(Debug, Deserialize)]
pub struct AdjudicateRequest {
    pub adj_offr_name: String,
    pub adj_offr_designation: String,
    pub adjudication_date: Option<String>,
    pub adjn_offr_remarks: Option<String>,
    pub rf_amount: Option<f64>,
    pub pp_amount: Option<f64>,
    pub ref_amount: Option<f64>,
    pub confiscated_value: Option<f64>,
    pub redeemed_value: Option<f64>,
    pub re_export_value: Option<f64>,
    pub close_case: Option<bool>,
    pub item_categories: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct CompleteOfflineRequest {
    pub adj_offr_name: String,
    pub adj_offr_designation: String,
    pub adjudication_date: Option<String>,
    pub adjn_offr_remarks: Option<String>,
    pub rf_amount: Option<f64>,
    pub pp_amount: Option<f64>,
    pub ref_amount: Option<f64>,
    pub confiscated_value: Option<f64>,
    pub redeemed_value: Option<f64>,
    pub re_export_value: Option<f64>,
    pub close_case: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct SidebarCountsResponse {
    pub pending: i64,
    pub offline_pending: i64,
}

#[derive(Debug, Deserialize)]
pub struct PostAdjRequest {
    pub post_adj_br_entries: Option<String>,
    pub post_adj_dr_no: Option<String>,
    pub post_adj_dr_date: Option<String>,
}
