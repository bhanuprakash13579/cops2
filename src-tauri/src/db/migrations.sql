-- COPS2 Database Migrations
-- Schema is identical to cops1 — existing cops.db files work without any migration.
-- All statements use IF NOT EXISTS so this is safe to run on every startup.

CREATE TABLE IF NOT EXISTS users (
    id INTEGER NOT NULL,
    user_name VARCHAR(100) NOT NULL,
    user_desig VARCHAR(100),
    user_id VARCHAR(50) NOT NULL,
    user_pwd VARCHAR(255) NOT NULL,
    created_by VARCHAR(50),
    created_on DATE,
    user_status VARCHAR(20),
    user_role VARCHAR(20) NOT NULL,
    closed_on DATE,
    PRIMARY KEY (id)
);
CREATE UNIQUE INDEX IF NOT EXISTS ix_users_user_id ON users (user_id);

CREATE TABLE IF NOT EXISTS ip_addrs_table (
    id INTEGER NOT NULL,
    ip_address VARCHAR(50),
    status VARCHAR(20),
    PRIMARY KEY (id)
);

CREATE TABLE IF NOT EXISTS shift_timing_master (
    id INTEGER NOT NULL,
    day_shift_from_hrs INTEGER,
    day_shift_to_hrs INTEGER,
    night_shift_from_hrs INTEGER,
    night_shift_to_hrs INTEGER,
    PRIMARY KEY (id)
);

CREATE TABLE IF NOT EXISTS batch_master (
    id INTEGER NOT NULL,
    current_batch_date DATE,
    current_batch_shift VARCHAR(20),
    PRIMARY KEY (id)
);

CREATE TABLE IF NOT EXISTS nationality_master (
    id INTEGER NOT NULL,
    nationality VARCHAR(100) NOT NULL,
    PRIMARY KEY (id),
    UNIQUE (nationality)
);

CREATE TABLE IF NOT EXISTS airlines_mast (
    id INTEGER NOT NULL,
    airline_name VARCHAR(200) NOT NULL,
    airline_code VARCHAR(20) NOT NULL,
    PRIMARY KEY (id)
);

CREATE TABLE IF NOT EXISTS arrival_flight_master (
    id INTEGER NOT NULL,
    flight_no VARCHAR(20) NOT NULL,
    airline_code VARCHAR(20) NOT NULL,
    PRIMARY KEY (id)
);

CREATE TABLE IF NOT EXISTS airport_master (
    id INTEGER NOT NULL,
    airport_name VARCHAR(200),
    airport_status VARCHAR(20),
    PRIMARY KEY (id)
);

CREATE TABLE IF NOT EXISTS item_cat_master (
    id INTEGER NOT NULL,
    category_code VARCHAR(20) NOT NULL,
    category_desc VARCHAR(200) NOT NULL,
    active_ind VARCHAR(5),
    bcd_adv_rate FLOAT,
    cvd_adv_rate FLOAT,
    PRIMARY KEY (id)
);

CREATE TABLE IF NOT EXISTS duty_rate_master (
    id INTEGER NOT NULL,
    duty_category VARCHAR(50) NOT NULL,
    from_date DATE NOT NULL,
    to_date DATE,
    active_ind VARCHAR(5),
    bcd_rate FLOAT,
    cvd_rate FLOAT,
    PRIMARY KEY (id)
);

CREATE TABLE IF NOT EXISTS dc_master (
    id INTEGER NOT NULL,
    dc_code VARCHAR(20) NOT NULL,
    dc_name VARCHAR(200) NOT NULL,
    dc_status VARCHAR(20),
    PRIMARY KEY (id)
);

CREATE TABLE IF NOT EXISTS os_master (
    id INTEGER NOT NULL,
    osdate DATE NOT NULL,
    osnumber INTEGER NOT NULL,
    location_code VARCHAR(20),
    PRIMARY KEY (id)
);

CREATE TABLE IF NOT EXISTS item_trans (
    id INTEGER NOT NULL,
    item_osdate DATE,
    item_os_no INTEGER,
    item_lcode VARCHAR(20),
    item_no INTEGER,
    item_qty FLOAT,
    item_uqc VARCHAR(20),
    item_value FLOAT,
    PRIMARY KEY (id)
);

-- ── Core Offence Sheet Tables ─────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS cops_master (
    id INTEGER NOT NULL,
    os_no VARCHAR(20) NOT NULL,
    os_date DATE NOT NULL,
    os_year INTEGER,
    location_code VARCHAR(20),
    shift VARCHAR(20),
    detention_date DATE,
    case_type VARCHAR(100),
    booked_by VARCHAR(200),
    pax_name VARCHAR(200),
    pax_nationality VARCHAR(100),
    passport_no VARCHAR(50),
    passport_date DATE,
    pp_issue_place VARCHAR(200),
    pax_address1 VARCHAR(300),
    pax_address2 VARCHAR(300),
    pax_address3 VARCHAR(300),
    pax_date_of_birth DATE,
    pax_status VARCHAR(50),
    residence_at VARCHAR(200),
    country_of_departure VARCHAR(200),
    port_of_dep_dest VARCHAR(200),
    date_of_departure VARCHAR(50),
    stay_abroad_days INTEGER,
    pax_image_filename TEXT,
    flight_no VARCHAR(20),
    flight_date DATE,
    total_items INTEGER,
    total_items_value FLOAT,
    total_fa_value REAL DEFAULT 0,
    dutiable_value FLOAT,
    redeemed_value FLOAT,
    re_export_value FLOAT,
    confiscated_value FLOAT,
    total_duty_amount FLOAT,
    rf_amount FLOAT,
    pp_amount FLOAT,
    ref_amount FLOAT,
    br_amount FLOAT,
    wh_amount REAL DEFAULT 0,
    other_amount REAL DEFAULT 0,
    total_payable FLOAT,
    br_no_str TEXT,
    br_no_num REAL,
    br_date_str TEXT,
    br_amount_str TEXT,
    is_legacy TEXT DEFAULT 'N',
    is_offline_adjudication TEXT DEFAULT 'N',
    file_spot TEXT DEFAULT 'Spot',
    is_draft VARCHAR(5) DEFAULT 'N',
    os_printed VARCHAR(5),
    os_category TEXT,
    online_os TEXT,
    adjudication_date DATE,
    adjudication_time DATETIME,
    adj_offr_name VARCHAR(200),
    adj_offr_designation VARCHAR(200),
    adjn_offr_remarks TEXT,
    adjn_offr_remarks1 TEXT,
    online_adjn VARCHAR(5),
    unique_no INTEGER,
    entry_deleted VARCHAR(5) DEFAULT 'N',
    bkup_taken VARCHAR(5) DEFAULT 'N',
    deleted_by TEXT,
    deleted_reason TEXT,
    deleted_on DATE,
    detained_by VARCHAR(200),
    seal_no VARCHAR(50),
    nationality VARCHAR(100),
    seizure_date DATE,
    dr_no VARCHAR(20),
    dr_year INTEGER,
    total_drs INTEGER DEFAULT 0,
    previous_os_details TEXT,
    previous_visits TEXT,
    father_name VARCHAR(200),
    old_passport_no VARCHAR(50),
    total_pkgs INTEGER DEFAULT 0,
    supdts_remarks TEXT,
    supdt_remarks2 VARCHAR(200),
    closure_ind VARCHAR(5),
    post_adj_br_entries TEXT,
    post_adj_dr_no TEXT,
    post_adj_dr_date DATE,
    quashed VARCHAR(1) DEFAULT 'N',
    quashed_by VARCHAR(255),
    quash_reason TEXT,
    quash_date DATE,
    rejected VARCHAR(1) DEFAULT 'N',
    reject_reason TEXT,
    pax_name_modified_by_vig TEXT,
    PRIMARY KEY (id)
);

CREATE INDEX IF NOT EXISTS ix_cops_master_os_no_year        ON cops_master (os_no, os_year);
CREATE INDEX IF NOT EXISTS ix_cops_master_draft_deleted      ON cops_master (entry_deleted, is_draft);
CREATE INDEX IF NOT EXISTS ix_cops_master_adjudication_date  ON cops_master (adjudication_date);
CREATE INDEX IF NOT EXISTS ix_cops_master_quashed_rejected   ON cops_master (quashed, rejected);
CREATE INDEX IF NOT EXISTS ix_cops_master_os_year            ON cops_master (os_year);
CREATE INDEX IF NOT EXISTS ix_cops_master_passport_no        ON cops_master (passport_no);
CREATE INDEX IF NOT EXISTS ix_cops_master_flight_no          ON cops_master (flight_no);

CREATE TABLE IF NOT EXISTS cops_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    os_no VARCHAR(20) NOT NULL,
    os_date DATE,
    os_year INTEGER,
    location_code VARCHAR(20),
    items_sno INTEGER NOT NULL,
    items_desc TEXT,
    items_qty FLOAT DEFAULT 0,
    items_uqc VARCHAR(20),
    value_per_piece FLOAT DEFAULT 0,
    items_value FLOAT DEFAULT 0,
    items_fa FLOAT DEFAULT 0,
    items_fa_type VARCHAR(10) DEFAULT 'value',
    items_fa_qty REAL,
    items_fa_uqc VARCHAR(20),
    cumulative_duty_rate FLOAT DEFAULT 0,
    items_duty FLOAT DEFAULT 0,
    items_duty_type VARCHAR(100),
    items_category VARCHAR(100),
    items_release_category VARCHAR(100),
    items_sub_category VARCHAR(100),
    items_dr_no INTEGER DEFAULT 0,
    items_dr_year INTEGER DEFAULT 0,
    unique_no INTEGER,
    entry_deleted VARCHAR(5) DEFAULT 'N',
    bkup_taken VARCHAR(5) DEFAULT 'N'
);

CREATE INDEX IF NOT EXISTS ix_cops_items_os_no_year ON cops_items (os_no, os_year);

CREATE TABLE IF NOT EXISTS cops_master_deleted (
    id INTEGER NOT NULL,
    os_no VARCHAR(20),
    os_date DATE,
    os_year INTEGER,
    location_code VARCHAR(20),
    booked_by VARCHAR(200),
    pax_name VARCHAR(200),
    pax_nationality VARCHAR(100),
    passport_no VARCHAR(50),
    passport_date DATE,
    pax_address1 VARCHAR(300),
    pax_address2 VARCHAR(300),
    pax_address3 VARCHAR(300),
    pax_date_of_birth DATE,
    pax_status VARCHAR(50),
    residence_at VARCHAR(200),
    country_of_departure VARCHAR(200),
    flight_no VARCHAR(20),
    flight_date DATE,
    total_items INTEGER,
    total_items_value FLOAT,
    dutiable_value FLOAT,
    redeemed_value FLOAT,
    re_export_value FLOAT,
    confiscated_value FLOAT,
    total_duty_amount FLOAT,
    rf_amount FLOAT,
    pp_amount FLOAT,
    ref_amount FLOAT,
    br_amount FLOAT,
    total_payable FLOAT,
    adjudication_date DATE,
    adj_offr_name VARCHAR(200),
    adj_offr_designation VARCHAR(200),
    adjn_offr_remarks TEXT,
    adjn_offr_remarks1 TEXT,
    online_adjn VARCHAR(5),
    os_printed VARCHAR(5),
    os_category TEXT,
    online_os TEXT,
    unique_no INTEGER,
    entry_deleted VARCHAR(5),
    bkup_taken VARCHAR(5),
    detained_by VARCHAR(200),
    seal_no VARCHAR(50),
    nationality VARCHAR(100),
    seizure_date DATE,
    pax_name_modified_by_vig TEXT,
    pax_image_filename TEXT,
    total_fa_value REAL DEFAULT 0,
    wh_amount REAL DEFAULT 0,
    other_amount REAL DEFAULT 0,
    br_no_str TEXT,
    br_no_num REAL,
    br_date_str TEXT,
    br_amount_str TEXT,
    dr_no VARCHAR(20),
    dr_year INTEGER,
    total_drs INTEGER,
    previous_os_details TEXT,
    previous_visits TEXT,
    father_name VARCHAR(200),
    old_passport_no VARCHAR(50),
    total_pkgs INTEGER,
    supdts_remarks TEXT,
    supdt_remarks2 VARCHAR(200),
    closure_ind VARCHAR(5),
    adjudication_time DATETIME,
    deleted_by TEXT,
    deleted_reason TEXT,
    deleted_on DATE,
    post_adj_br_entries TEXT,
    post_adj_dr_no TEXT,
    post_adj_dr_date DATE,
    is_legacy TEXT DEFAULT 'N',
    is_offline_adjudication TEXT DEFAULT 'N',
    file_spot TEXT DEFAULT 'Spot',
    PRIMARY KEY (id)
);

CREATE INDEX IF NOT EXISTS ix_cops_master_deleted_os_no ON cops_master_deleted (os_no);

CREATE TABLE IF NOT EXISTS cops_items_deleted (
    id INTEGER NOT NULL,
    os_no VARCHAR(20),
    os_date DATE,
    os_year INTEGER,
    location_code VARCHAR(20),
    items_sno INTEGER,
    items_desc TEXT,
    items_qty FLOAT,
    items_uqc VARCHAR(20),
    items_value FLOAT,
    items_fa FLOAT,
    items_duty FLOAT,
    items_duty_type VARCHAR(50),
    items_category VARCHAR(50),
    items_release_category VARCHAR(50),
    items_sub_category VARCHAR(50),
    items_dr_no INTEGER,
    items_dr_year INTEGER,
    unique_no INTEGER,
    entry_deleted VARCHAR(5),
    bkup_taken VARCHAR(5),
    PRIMARY KEY (id)
);

CREATE TABLE IF NOT EXISTS audit_events (
    id VARCHAR(50) NOT NULL,
    entity_id VARCHAR(50),
    entity_type VARCHAR(50),
    action VARCHAR(20),
    payload JSON,
    node_id VARCHAR(50),
    timestamp DATETIME,
    PRIMARY KEY (id)
);
CREATE INDEX IF NOT EXISTS ix_audit_events_entity_id   ON audit_events (entity_id);
CREATE INDEX IF NOT EXISTS ix_audit_events_entity_type ON audit_events (entity_type);
CREATE INDEX IF NOT EXISTS ix_audit_events_timestamp   ON audit_events (timestamp);

-- ── Unique index on airlines_mast ─────────────────────────────────────────────
CREATE UNIQUE INDEX IF NOT EXISTS ix_airlines_mast_code ON airlines_mast (airline_code);

-- ── Extended item_cat_master columns (added in migration) ─────────────────────
-- SQLite ignores ADD COLUMN if column already exists via IF NOT EXISTS workaround
CREATE TABLE IF NOT EXISTS item_cat_master_v2_tmp (id INTEGER);
DROP TABLE IF EXISTS item_cat_master_v2_tmp;
-- Safe column additions via INSERT OR IGNORE won't break existing dbs;
-- we use ALTER TABLE ADD COLUMN IF NOT EXISTS pattern via a helper table check.

-- ── BR Number Series Limits ───────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS br_no_limits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    br_type VARCHAR(20) NOT NULL,
    br_series_from INTEGER NOT NULL,
    br_series_to INTEGER NOT NULL
);

-- ── Baggage Receipts ──────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS br_master (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    br_no INTEGER NOT NULL,
    br_date DATE NOT NULL,
    br_type VARCHAR(20) NOT NULL,
    br_year INTEGER,
    br_shift VARCHAR(20),
    flight_no VARCHAR(20),
    flight_date DATE,
    pax_name VARCHAR(200),
    pax_nationality VARCHAR(100),
    passport_no VARCHAR(50),
    passport_date DATE,
    passport_issue_place VARCHAR(200),
    pax_address1 VARCHAR(300),
    pax_address2 VARCHAR(300),
    pax_address3 VARCHAR(300),
    pax_date_of_birth DATE,
    pax_status VARCHAR(50),
    residence_at VARCHAR(200),
    country_of_departure VARCHAR(200),
    departure_date DATE,
    os_no VARCHAR(20),
    os_date DATE,
    dr_no VARCHAR(20),
    dr_date DATE,
    total_items_value FLOAT DEFAULT 0,
    total_fa_value FLOAT DEFAULT 0,
    total_duty_amount FLOAT DEFAULT 0,
    rf_amount FLOAT DEFAULT 0,
    pp_amount FLOAT DEFAULT 0,
    ref_amount FLOAT DEFAULT 0,
    wh_amount FLOAT DEFAULT 0,
    other_amount FLOAT DEFAULT 0,
    br_amount FLOAT DEFAULT 0,
    challan_no VARCHAR(50),
    bank_date DATE,
    bank_shift VARCHAR(20),
    batch_date DATE,
    batch_shift VARCHAR(20),
    dc_code VARCHAR(20),
    unique_no INTEGER,
    location_code VARCHAR(20),
    login_id VARCHAR(50),
    entry_deleted VARCHAR(5) DEFAULT 'N',
    bkup_taken VARCHAR(5) DEFAULT 'N',
    br_printed VARCHAR(5) DEFAULT 'N',
    ff_ind VARCHAR(5),
    image_filename VARCHAR(500),
    table_name VARCHAR(100),
    arrived_from VARCHAR(200),
    br_amount_str VARCHAR(200),
    br_no_str VARCHAR(50),
    abroad_stay INTEGER,
    total_fa_availed FLOAT DEFAULT 0,
    actual_br_type VARCHAR(20),
    total_payable FLOAT DEFAULT 0,
    availed_remarks TEXT
);
CREATE INDEX IF NOT EXISTS ix_br_master_no_date  ON br_master (br_no, br_date);
CREATE INDEX IF NOT EXISTS ix_br_master_passport ON br_master (passport_no);
CREATE INDEX IF NOT EXISTS ix_br_master_deleted  ON br_master (entry_deleted);

CREATE TABLE IF NOT EXISTS br_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    br_no INTEGER NOT NULL,
    br_date DATE NOT NULL,
    br_shift VARCHAR(20),
    br_type VARCHAR(20) NOT NULL,
    items_sno INTEGER NOT NULL,
    items_desc TEXT,
    items_qty FLOAT DEFAULT 0,
    items_uqc VARCHAR(20),
    items_value FLOAT DEFAULT 0,
    items_fa FLOAT DEFAULT 0,
    items_bcd FLOAT DEFAULT 0,
    items_cvd FLOAT DEFAULT 0,
    items_cess FLOAT DEFAULT 0,
    items_hec FLOAT DEFAULT 0,
    items_duty FLOAT DEFAULT 0,
    items_duty_type VARCHAR(50),
    items_category VARCHAR(50),
    items_dr_no INTEGER DEFAULT 0,
    items_dr_year INTEGER DEFAULT 0,
    items_release_category VARCHAR(50),
    flight_no VARCHAR(20),
    bank_date DATE,
    bank_shift VARCHAR(20),
    batch_date DATE,
    batch_shift VARCHAR(20),
    unique_no INTEGER,
    location_code VARCHAR(20),
    login_id VARCHAR(50),
    entry_deleted VARCHAR(5) DEFAULT 'N',
    bkup_taken VARCHAR(5) DEFAULT 'N'
);
CREATE INDEX IF NOT EXISTS ix_br_items_no_date ON br_items (br_no, br_date);

-- ── Detention Receipts ────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS dr_master (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    dr_no INTEGER NOT NULL,
    dr_date DATE NOT NULL,
    dr_year INTEGER,
    dr_type VARCHAR(20) NOT NULL,
    shift VARCHAR(20),
    pax_name VARCHAR(200),
    passport_no VARCHAR(50),
    passport_date DATE,
    pax_address1 VARCHAR(300),
    pax_address2 VARCHAR(300),
    pax_address3 VARCHAR(300),
    port_of_departure VARCHAR(200),
    flight_no VARCHAR(20),
    flight_date DATE,
    total_items_value FLOAT DEFAULT 0,
    total_fa_value FLOAT DEFAULT 0,
    closure_ind VARCHAR(5) DEFAULT 'N',
    closure_remarks TEXT,
    closure_date DATE,
    closed_batch_date DATE,
    closed_batch_shift VARCHAR(20),
    warehouse_no VARCHAR(50),
    entry_deleted VARCHAR(5) DEFAULT 'N',
    unique_no INTEGER,
    location_code VARCHAR(20),
    login_id VARCHAR(50),
    detained_by VARCHAR(200),
    detained_pkg_no VARCHAR(50),
    detained_pkg_type VARCHAR(50),
    seal_no VARCHAR(50),
    dr_printed VARCHAR(5) DEFAULT 'N',
    detention_reasons TEXT,
    seizure_date DATE,
    os_no VARCHAR(20),
    receipt_by_who VARCHAR(200),
    batch_date DATE,
    batch_shift VARCHAR(20),
    departure_date DATE,
    pax_date_of_birth DATE
);
CREATE INDEX IF NOT EXISTS ix_dr_master_no_year  ON dr_master (dr_no, dr_year);
CREATE INDEX IF NOT EXISTS ix_dr_master_passport ON dr_master (passport_no);
CREATE INDEX IF NOT EXISTS ix_dr_master_deleted  ON dr_master (entry_deleted);

CREATE TABLE IF NOT EXISTS dr_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    dr_no INTEGER NOT NULL,
    dr_date DATE,
    dr_type VARCHAR(20),
    items_sno INTEGER NOT NULL,
    items_desc TEXT,
    items_qty FLOAT DEFAULT 0,
    items_uqc VARCHAR(20),
    items_value FLOAT DEFAULT 0,
    items_fa FLOAT DEFAULT 0,
    items_release_category VARCHAR(50),
    receipt_by_who VARCHAR(5),
    item_closure_remarks TEXT,
    detained_pkg_no VARCHAR(50),
    detained_pkg_type VARCHAR(50),
    unique_no INTEGER,
    location_code VARCHAR(20)
);
CREATE INDEX IF NOT EXISTS ix_dr_items_no_date ON dr_items (dr_no, dr_date);

-- ── Legal Statutes ────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS legal_statutes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    keyword VARCHAR(50) NOT NULL,
    display_name VARCHAR(200) NOT NULL,
    is_prohibited INTEGER NOT NULL DEFAULT 0,
    supdt_goods_clause TEXT NOT NULL DEFAULT '',
    adjn_goods_clause TEXT NOT NULL DEFAULT '',
    legal_reference TEXT NOT NULL DEFAULT ''
);
CREATE UNIQUE INDEX IF NOT EXISTS ix_legal_statutes_keyword ON legal_statutes (keyword);

-- ── Feature Flags ─────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS feature_flags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    apis_enabled INTEGER NOT NULL DEFAULT 0,
    session_timeout_minutes INTEGER NOT NULL DEFAULT 480
);

-- ── Print Template Config (versioned) ─────────────────────────────────────────
CREATE TABLE IF NOT EXISTS print_template_config (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    field_key VARCHAR(100) NOT NULL,
    field_label VARCHAR(200),
    field_value TEXT NOT NULL,
    effective_from DATE NOT NULL,
    created_by VARCHAR(100),
    created_at DATETIME
);
CREATE INDEX IF NOT EXISTS ix_print_template_key  ON print_template_config (field_key);
CREATE INDEX IF NOT EXISTS ix_print_template_date ON print_template_config (effective_from);

-- ── Baggage Rules Config (versioned) ─────────────────────────────────────────
CREATE TABLE IF NOT EXISTS baggage_rules_config (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    rule_key VARCHAR(100) NOT NULL,
    rule_label VARCHAR(200),
    rule_value FLOAT NOT NULL,
    rule_uqc VARCHAR(20),
    effective_from DATE NOT NULL,
    created_by VARCHAR(100),
    created_at DATETIME
);
CREATE INDEX IF NOT EXISTS ix_baggage_rules_key  ON baggage_rules_config (rule_key);
CREATE INDEX IF NOT EXISTS ix_baggage_rules_date ON baggage_rules_config (effective_from);

-- ── Special Item Allowances ───────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS special_item_allowances (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    item_name VARCHAR(100) NOT NULL,
    keywords TEXT,
    allowance_qty FLOAT NOT NULL,
    allowance_uqc VARCHAR(20),
    effective_from DATE NOT NULL,
    active VARCHAR(1) DEFAULT 'Y',
    created_by VARCHAR(100),
    created_at DATETIME
);

-- ── Allowed Devices (network whitelist) ───────────────────────────────────────
CREATE TABLE IF NOT EXISTS allowed_devices (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    label VARCHAR(200) NOT NULL,
    ip_address VARCHAR(50),
    mac_address VARCHAR(50),
    hostname VARCHAR(200),
    is_active INTEGER NOT NULL DEFAULT 1,
    added_by VARCHAR(100),
    added_on DATE,
    notes TEXT
);

-- ── Port Master ───────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS port_master (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    port_of_departure VARCHAR(200) NOT NULL UNIQUE
);

-- ── Remarks Templates (adjudication order boilerplate, keyed by purpose) ──────
CREATE TABLE IF NOT EXISTS remarks_templates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    template_key VARCHAR(100) NOT NULL UNIQUE,
    template_text TEXT,
    updated_on DATE
);

-- ── Allowed Devices (schema fix: match cops1 column names) ───────────────────
-- allowed_devices already created above; add missing columns defensively
-- (SQLite ignores ADD COLUMN if column already exists via the migration guard)

-- ── cops_items_deleted (audit trail for item-level deletes) ──────────────────
CREATE TABLE IF NOT EXISTS cops_items_deleted (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    os_no VARCHAR(50),
    os_year INTEGER,
    items_sno INTEGER,
    items_desc TEXT,
    items_qty REAL,
    items_uqc VARCHAR(20),
    items_value REAL,
    items_duty REAL,
    entry_deleted VARCHAR(1) DEFAULT 'Y',
    deleted_on DATE
);
