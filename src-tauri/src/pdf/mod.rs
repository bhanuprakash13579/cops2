use anyhow::Result;
use serde_json::Value;
use typst::{
    diag::{FileError, FileResult},
    foundations::{Bytes, Datetime},
    syntax::{FileId, Source, VirtualPath},
    text::{Font, FontBook},
    utils::LazyHash,
    Library, World,
};

// ── Typst World ───────────────────────────────────────────────────────────────

struct OsWorld {
    library: LazyHash<Library>,
    book:    LazyHash<FontBook>,
    fonts:   Vec<Font>,
    source:  Source,
}

impl OsWorld {
    fn new(source_text: String) -> Self {
        let mut book  = FontBook::new();
        let mut fonts = Vec::new();

        for data in typst_assets::fonts() {
            let bytes = Bytes::new(data);
            for i in 0u32.. {
                match Font::new(bytes.clone(), i) {
                    Some(font) => {
                        book.push(font.info().clone());
                        fonts.push(font);
                    }
                    None => break,
                }
            }
        }

        let id     = FileId::new(None, VirtualPath::new("/main.typ"));
        let source = Source::new(id, source_text);

        OsWorld {
            library: LazyHash::new(Library::default()),
            book:    LazyHash::new(book),
            fonts,
            source,
        }
    }
}

impl World for OsWorld {
    fn library(&self) -> &LazyHash<Library> { &self.library }
    fn book(&self)    -> &LazyHash<FontBook> { &self.book }
    fn main(&self)    -> FileId              { self.source.id() }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.source.id() { Ok(self.source.clone()) }
        else { Err(FileError::NotFound(id.vpath().as_rootless_path().to_path_buf())) }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        Err(FileError::NotFound(id.vpath().as_rootless_path().to_path_buf()))
    }

    fn font(&self, index: usize) -> Option<Font> { self.fonts.get(index).cloned() }

    fn today(&self, _offset: Option<i64>) -> Option<Datetime> { None }
}

// ── Text helpers ──────────────────────────────────────────────────────────────

/// Escape special characters for Typst content blocks.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '#'  => out.push_str("\\#"),
            '['  => out.push_str("\\["),
            ']'  => out.push_str("\\]"),
            '$'  => out.push_str("\\$"),
            '*'  => out.push_str("\\*"),
            '_'  => out.push_str("\\_"),
            '@'  => out.push_str("\\@"),
            '<'  => out.push_str("\\<"),
            '>'  => out.push_str("\\>"),
            c    => out.push(c),
        }
    }
    out
}

fn str_val(v: &Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}
fn f64_val(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0)
}
fn i64_val(v: &Value, k: &str) -> i64 {
    v.get(k).and_then(|x| x.as_i64()).unwrap_or(0)
}

fn fmt_indian(n: f64) -> String {
    let n_int = n.round() as i64;
    if n_int == 0 { return "0".to_string(); }
    let s = n_int.unsigned_abs().to_string();
    if s.len() <= 3 { return s; }
    let tail  = &s[s.len() - 3..];
    let front = &s[..s.len() - 3];
    let mut parts: Vec<&str> = Vec::new();
    let mut i = front.len();
    while i > 0 {
        let start = if i > 2 { i - 2 } else { 0 };
        parts.push(&front[start..i]);
        i = start;
    }
    parts.reverse();
    format!("{},{}", parts.join(","), tail)
}

fn fmt_num(n: f64) -> String {
    if n == 0.0 { String::new() } else { fmt_indian(n) }
}

fn fmt_qty(n: f64) -> String {
    if n.fract() == 0.0 { format!("{}", n as i64) } else { format!("{n:.2}").trim_end_matches('0').trim_end_matches('.').to_string() }
}

fn num_to_words(n: i64) -> String {
    if n == 0 { return "Zero".to_string(); }
    let a = ["", "One ", "Two ", "Three ", "Four ", "Five ", "Six ", "Seven ",
             "Eight ", "Nine ", "Ten ", "Eleven ", "Twelve ", "Thirteen ",
             "Fourteen ", "Fifteen ", "Sixteen ", "Seventeen ", "Eighteen ", "Nineteen "];
    let b = ["", "", "Twenty", "Thirty", "Forty", "Fifty",
             "Sixty", "Seventy", "Eighty", "Ninety"];
    if n < 20  { return a[n as usize].trim_end().to_string(); }
    if n < 100 {
        let t = b[(n / 10) as usize];
        let u = a[(n % 10) as usize].trim_end();
        return if u.is_empty() { t.to_string() } else { format!("{t} {u}") };
    }
    if n < 1_000 {
        let h = a[(n / 100) as usize].trim_end();
        let r = n % 100;
        return if r == 0 { format!("{h} Hundred") } else { format!("{h} Hundred and {}", num_to_words(r)) };
    }
    if n < 1_00_000 {
        let k = n / 1_000;
        let r = n % 1_000;
        return if r == 0 { format!("{} Thousand", num_to_words(k)) }
               else { format!("{} Thousand {}", num_to_words(k), num_to_words(r)) };
    }
    if n < 1_00_00_000 {
        let l = n / 1_00_000;
        let r = n % 1_00_000;
        return if r == 0 { format!("{} Lakh", num_to_words(l)) }
               else { format!("{} Lakh {}", num_to_words(l), num_to_words(r)) };
    }
    let c = n / 1_00_00_000;
    let r = n % 1_00_00_000;
    if r == 0 { format!("{} Crore", num_to_words(c)) }
    else { format!("{} Crore {}", num_to_words(c), num_to_words(r)) }
}

fn title_words(n: f64) -> String { num_to_words(n.round() as i64) }

fn uqc_label(code: &str) -> &str {
    match code.to_uppercase().as_str() {
        "NOS" => "Nos.", "STK" => "Sticks", "KGS" => "Kgs.",
        "GMS" => "Gms.", "LTR" => "Ltrs.", "MTR" => "Mtrs.", "PRS" => "Pairs",
        other => if other.is_empty() { "Nos." } else { code },
    }
}

fn day_or_night(shift: &str) -> &str {
    let s = shift.to_uppercase();
    if s.contains('A') || s.contains('B') || s.contains("DAY") { "(D)" }
    else if s.contains('C') || s.contains('D') || s.contains("NIGHT") { "(N)" }
    else { "" }
}

fn eff_fa(item_value: f64, item: &Value) -> f64 {
    let cat = str_val(item, "items_release_category").to_uppercase();
    if !matches!(cat.as_str(), "UNDER DUTY" | "UNDER OS" | "RF" | "REF") {
        return 0.0;
    }
    if str_val(item, "items_fa_type") == "qty" {
        let total_qty = f64_val(item, "items_qty");
        let fa_qty    = f64_val(item, "items_fa_qty");
        if total_qty > 0.0 { (fa_qty / total_qty * item_value).min(item_value) } else { 0.0 }
    } else {
        f64_val(item, "items_fa")
    }
}

fn fmt_br_entries(raw: &str) -> String {
    if raw.is_empty() { return String::new(); }
    if let Ok(arr) = serde_json::from_str::<Vec<Value>>(raw) {
        arr.iter().map(|e| {
            let mut parts = Vec::new();
            let br_no  = e.get("br_no").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
            let br_date= e.get("br_date").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
            let br_amt = e.get("br_amount").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
            if !br_no.is_empty()   { parts.push(format!("BR No. {br_no}")); }
            if !br_date.is_empty() { parts.push(format!("Dt. {br_date}")); }
            if !br_amt.is_empty()  { parts.push(format!("Rs. {br_amt}")); }
            parts.join(" / ")
        }).collect::<Vec<_>>().join("\n")
    } else { raw.to_string() }
}

fn fmt_dr(dr_no: &str, dr_date: &str) -> String {
    let mut parts = Vec::new();
    if !dr_no.is_empty()   { parts.push(format!("DR No. {dr_no}")); }
    if !dr_date.is_empty() { parts.push(format!("Dt. {dr_date}")); }
    parts.join(" / ")
}

fn slnos_text(slnos: &[String]) -> String {
    if slnos.is_empty() { String::new() }
    else { format!(" at Sl.No(s). {}", slnos.join(", ")) }
}

// ── Typst source builder ──────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
fn build_typst_source(case: &Value) -> String {
    // ── Extract master fields ─────────────────────────────────────────────────
    let os_no    = str_val(case, "os_no");
    let os_year  = i64_val(case, "os_year");
    let booked_by= str_val(case, "booked_by");
    let os_date  = str_val(case, "os_date");
    let pax_name = str_val(case, "pax_name");
    let father_name = str_val(case, "father_name");
    let addr_parts: Vec<String> = ["pax_address1","pax_address2","pax_address3"]
        .iter().map(|k| str_val(case, k)).filter(|s| !s.is_empty()).collect();
    let pax_address = addr_parts.join(", ");
    let mut pax_name_addr = pax_name.clone();
    if !father_name.is_empty() { pax_name_addr.push_str(&format!(", {father_name}")); }
    if !pax_address.is_empty() { pax_name_addr.push_str(&format!(", {pax_address}")); }

    let passport_no   = str_val(case, "passport_no");
    let passport_date = str_val(case, "passport_date");
    let flight_no     = str_val(case, "flight_no");
    let flight_date   = str_val(case, "flight_date");
    let port_dest     = str_val(case, "port_of_dep_dest");
    let country_dep   = str_val(case, "country_of_departure");
    let nationality   = {
        let n = str_val(case, "nationality");
        if n.is_empty() { str_val(case, "pax_nationality") } else { n }
    };
    let date_of_dep   = { let d = str_val(case, "date_of_departure"); if d.is_empty() { "N.A.".into() } else { d } };
    let stay_abroad   = i64_val(case, "stay_abroad_days");
    let residence_at  = {
        let r = str_val(case, "residence_at");
        if r.is_empty() { let c = str_val(case, "country_of_departure"); if c.is_empty() { "ABROAD".into() } else { c } } else { r }
    };
    let previous_visits = { let v = str_val(case, "previous_visits"); if v.is_empty() { "NIL".into() } else { v } };
    let supdts_remarks  = { let r = str_val(case, "supdts_remarks"); if r.is_empty() { "NIL".into() } else { r } };

    let case_type  = str_val(case, "case_type").to_uppercase();
    let is_export  = case_type.contains("EXPORT");
    let from_to    = if is_export {
        format!("CHENNAI TO {}", if port_dest.is_empty() { &country_dep } else { &port_dest })
    } else {
        format!("{} TO CHENNAI", if port_dest.is_empty() { &country_dep } else { &port_dest })
    };
    let stay_text = if is_export { "N/A".to_string() } else { format!("{stay_abroad} Days") };

    // Adjudication fields
    let adj_name  = { let n = str_val(case, "adj_offr_name"); if n.is_empty() { "__________________________".into() } else { n } };
    let adj_desig = { let d = str_val(case, "adj_offr_designation"); if d.is_empty() { "Deputy/Asst.Commr.".into() } else { d } };
    let adj_date  = { let d = str_val(case, "adjudication_date"); if d.is_empty() { os_date.clone() } else { d } };
    let adj_remarks = { let r = str_val(case, "adjn_offr_remarks"); if r.is_empty() { "No remarks provided.".into() } else { r } };
    let shift_val = str_val(case, "shift");
    let day_night = day_or_night(&shift_val);

    // Money fields
    let rf_amount  = f64_val(case, "rf_amount");
    let ref_amount = f64_val(case, "ref_amount");
    let pp_amount  = f64_val(case, "pp_amount");

    // BR / DR
    let br_display = esc(&fmt_br_entries(&str_val(case, "post_adj_br_entries")));
    let dr_display = esc(&fmt_dr(&str_val(case, "post_adj_dr_no"), &str_val(case, "post_adj_dr_date")));

    // Previous offences (use stored fields — no live cross-query in PDF)
    let prev_offence_display = { let v = str_val(case, "previous_visits"); if v.is_empty() { "NIL".into() } else { v } };
    let other_pp_offences    = { let v = str_val(case, "previous_os_details"); if v.is_empty() { "NIL".into() } else { v } };

    // ── Items pass — compute per-item display + summary totals ────────────────
    let empty_items = Value::Array(vec![]);
    let items = case.get("items").and_then(|x| x.as_array()).unwrap_or(items_ref(&empty_items));

    let liable_cats = ["CONFS","ABS_CONFS","RE_EXP","RF","REF","UNDER OS"];
    let fa_sum_cats = ["UNDER DUTY","UNDER OS","RF","REF"];

    let mut total_fa_monetary = 0.0_f64;
    let mut total_dutiable    = 0.0_f64;
    let mut total_liable_val  = 0.0_f64;
    let mut rf_val_items      = 0.0_f64;
    let mut ref_val_items     = 0.0_f64;
    let mut confs_val_items   = 0.0_f64;
    let mut qty_fa_list: Vec<String>  = Vec::new();
    let mut rf_slnos:    Vec<String>  = Vec::new();
    let mut ref_slnos:   Vec<String>  = Vec::new();
    let mut confs_slnos: Vec<String>  = Vec::new();

    struct ItemRow {
        sno: usize, desc: String, qty_str: String,
        fa_disp: String, duty_disp: String, total_disp: String,
    }
    let mut item_rows: Vec<ItemRow> = Vec::new();

    for (idx, item) in items.iter().enumerate() {
        let cat = str_val(item, "items_release_category").to_uppercase();
        let cat = if cat.is_empty() { "UNDER OS".to_string() } else { cat };

        let val       = f64_val(item, "items_value");
        let fa        = eff_fa(val, item);
        let dutiable  = (val - fa).max(0.0);
        let fa_type   = str_val(item, "items_fa_type");
        let qty_val   = f64_val(item, "items_qty");
        let uqc       = uqc_label(&str_val(item, "items_uqc")).to_string();
        let sno       = idx + 1;

        // Accumulate totals
        if fa_sum_cats.contains(&cat.as_str()) {
            if fa_type == "qty" {
                let fa_qty = f64_val(item, "items_fa_qty");
                if fa_qty > 0.0 {
                    let fa_uqc_raw = str_val(item, "items_fa_uqc");
                    let fa_uqc = uqc_label(&fa_uqc_raw);
                    qty_fa_list.push(format!("{} {} of {}", fmt_qty(fa_qty), fa_uqc, str_val(item, "items_desc")));
                }
            } else {
                total_fa_monetary += fa;
            }
        }
        if cat == "UNDER DUTY" { total_dutiable += dutiable; }
        if liable_cats.contains(&cat.as_str()) { total_liable_val += dutiable; }
        if cat == "RF"    { rf_val_items   += dutiable; rf_slnos.push(sno.to_string()); }
        if cat == "REF"   { ref_val_items  += dutiable; ref_slnos.push(sno.to_string()); }
        if cat == "CONFS" { confs_val_items+= dutiable; confs_slnos.push(sno.to_string()); }

        // FA display
        let show_fa = matches!(cat.as_str(), "UNDER DUTY" | "UNDER OS" | "RF" | "REF");
        let fa_disp = if !show_fa {
            "---".to_string()
        } else if fa_type == "qty" {
            let fa_qty = f64_val(item, "items_fa_qty");
            let fa_uqc_raw = str_val(item, "items_fa_uqc");
            let fa_uqc = uqc_label(&fa_uqc_raw);
            if fa_qty > 0.0 { format!("{} {}", fmt_qty(fa_qty), fa_uqc) } else { "---".to_string() }
        } else {
            if fa > 0.0 { fmt_indian(fa) } else { "---".to_string() }
        };

        let duty_disp  = if cat == "UNDER DUTY" && dutiable > 0.0 { fmt_indian(dutiable) } else { "---".to_string() };
        let total_disp = if liable_cats.contains(&cat.as_str()) && dutiable > 0.0 { fmt_indian(dutiable) } else { "---".to_string() };
        let desc_str   = str_val(item, "items_desc");

        item_rows.push(ItemRow {
            sno,
            desc: esc(&desc_str.to_uppercase()),
            qty_str: format!("{} {}", fmt_qty(qty_val), esc(&uqc)),
            fa_disp: esc(&fa_disp),
            duty_disp: esc(&duty_disp),
            total_disp: esc(&total_disp),
        });
    }

    // Summary values
    let master_redeemed  = f64_val(case, "redeemed_value");
    let master_re_export = f64_val(case, "re_export_value");
    let master_confs     = f64_val(case, "confiscated_value");

    let mut conf_value     = if master_redeemed  > 0.0 { master_redeemed  } else { rf_val_items    };
    let re_exp_value       = if master_re_export > 0.0 { master_re_export } else { ref_val_items   };
    let mut abs_conf_value = if master_confs     > 0.0 { master_confs     } else { confs_val_items };

    // RF items with zero redemption fine → absolute confiscation
    if rf_amount == 0.0 && conf_value > 0.0 { abs_conf_value += conf_value; conf_value = 0.0; }
    let all_abs_conf_slnos: Vec<String> = if rf_amount == 0.0 && !rf_slnos.is_empty() {
        let mut merged = confs_slnos.clone();
        merged.extend(rf_slnos.iter().cloned());
        merged.sort_by_key(|s| s.parse::<usize>().unwrap_or(0));
        merged
    } else { confs_slnos.clone() };

    let total_items_value = if total_liable_val > 0.0 { total_liable_val } else { f64_val(case, "total_items_value") };

    let qty_fa_str = qty_fa_list.join(" & ");
    let fa_qty_note = if !qty_fa_str.is_empty() {
        format!(" #text(size: 7pt)[(along with {})]", esc(&qty_fa_str))
    } else { String::new() };

    // ── Order paragraphs ──────────────────────────────────────────────────────
    let para_rf = if conf_value > 0.0 && rf_amount > 0.0 {
        let slnos_txt = esc(&slnos_text(&rf_slnos));
        if is_export {
            format!("I Order confiscation of the goods{slnos_txt} valued at Rs.{}/- under Section 113 of the Customs Act, 1962, but allow the passenger an option to redeem the goods valued at Rs.{}/- on a fine of Rs.{}/- (Rupees {} Only) in lieu of confiscation under Section 125 of the Customs Act 1962 within 7 days from the date of receipt of this Order.",
                conf_value as i64, conf_value as i64, rf_amount as i64, esc(&title_words(rf_amount)))
        } else {
            format!("I Order confiscation of the goods{slnos_txt} valued at Rs.{}/- under Section 111(d), (i), (l), (m) \\& (o) of the Customs Act, 1962 read with Section 3(3) of Foreign Trade (D\\&R) Act, 1992, but allow the passenger an option to redeem the goods valued at Rs.{}/- on a fine of Rs.{}/- (Rupees {} Only) in lieu of confiscation under Section 125 of the Customs Act 1962 within 7 days from the date of receipt of this Order, Duty extra.",
                conf_value as i64, conf_value as i64, rf_amount as i64, esc(&title_words(rf_amount)))
        }
    } else { String::new() };

    let para_ref = if re_exp_value > 0.0 && ref_amount > 0.0 && !is_export {
        let slnos_txt = esc(&slnos_text(&ref_slnos));
        format!("However, I give an option to reship the goods{slnos_txt} valued at Rs.{}/- on a fine of Rs.{}/- (Rupees {} Only) under Section 125 of the Customs Act 1962 within 1 Month from the date of this Order.",
            re_exp_value as i64, ref_amount as i64, esc(&title_words(ref_amount)))
    } else { String::new() };

    let para_abs_conf = if abs_conf_value > 0.0 {
        let also = if conf_value > 0.0 || re_exp_value > 0.0 { "also " } else { "" };
        let slnos_txt = esc(&slnos_text(&all_abs_conf_slnos));
        if is_export {
            format!("I {also}order absolute confiscation of the goods{slnos_txt} valued at Rs.{}/- under Section 113 of the Customs Act, 1962.", abs_conf_value as i64)
        } else {
            format!("I {also}order absolute confiscation of the goods{slnos_txt} valued at Rs.{}/- under Section 111(d), (i), (l), (m) \\& (o) of the Customs Act, 1962 read with Section 3(3) of the Foreign Trade (D\\&R) Act, 1992.", abs_conf_value as i64)
        }
    } else { String::new() };

    let para_pp = if pp_amount > 0.0 {
        if is_export {
            format!("I further impose a Personal Penalty of Rs.{}/- (Rupees {} Only) under Section 114 of the Customs Act, 1962.", pp_amount as i64, esc(&title_words(pp_amount)))
        } else {
            format!("I further impose a Personal Penalty of Rs.{}/- (Rupees {} Only) under Section 112(a) of the Customs Act, 1962.", pp_amount as i64, esc(&title_words(pp_amount)))
        }
    } else { String::new() };

    // ── Build item rows markup ────────────────────────────────────────────────
    let mut rows_markup = String::new();
    for row in &item_rows {
        rows_markup.push_str(&format!(
            "[{}], [{}], [{}], [{}], [{}], [{}],\n",
            row.sno, row.desc, row.qty_str, row.fa_disp, row.duty_disp, row.total_disp
        ));
    }
    // Pad with empty rows if fewer than 5 items (keep table looking full)
    for _ in item_rows.len()..5 {
        rows_markup.push_str("[], [], [], [], [], [],\n");
    }

    // ── Versioned config defaults ─────────────────────────────────────────────
    let office_hdr1   = "Office of the Deputy / Asst. Commissioner of Customs";
    let office_hdr2   = "(Airport), Anna International Airport, Chennai-600027";
    let page1_title   = if is_export { "DETENTION / SEIZURE OF PASSENGER'S BAGGAGE (EXPORT)" }
                        else { "DETENTION / SEIZURE OF PASSENGER'S BAGGAGE" };
    let inv_heading   = if is_export { "INVENTORY OF THE GOODS DETAINED FOR EXPORT" }
                        else { "INVENTORY OF THE GOODS IMPORTED" };
    let col_fa_hdr    = "Goods Allowed Free Under Rule 5 / Rule 13 of Baggage Rules, 1994";
    let col_duty_hdr  = "Goods Passed On Duty";
    let col_liable_hdr= "Goods Liable to Action Under FEMA / Foreign Trade Act, 1992 & Customs Act, 1962";
    let sum_duty_txt  = "Value of Goods Charged to Duty Under Foreign Trade (D&R) Act, 1992 & Customs Act, 1962";
    let sum_liab_txt  = "Value of Goods Liable to Action under FEMA / Foreign Trade (D&R) Act, 1992 & Customs Act 1962";
    let supdt_sig     = "Supdt. of Customs";
    let p2_office_hdr = "Office of the Deputy / Asst. Commissioner of Customs (Airport), Anna International airport, Chennai-600027.";
    let waiver_hdr    = "WAIVER OF SHOW CAUSE NOTICE";
    let waiver_txt1   = if is_export {
        "The Charges have been orally communicated to me in respect of the goods mentioned overleaf and detained at the time of my departure. Orders in the case may please be passed without issue of Show Cause Notice. However I may kindly be given a Personal Hearing."
    } else {
        "The Charges have been orally communicated to me in respect of the goods mentioned overleaf and imported by me. Orders in the case may please be passed without issue of Show Cause Notice. However I may kindly be given a Personal Hearing."
    };
    let waiver_txt2   = "I was present during the personal hearing conducted by the Deputy / Asst. Commissioner and I was heard.";
    let record_hdr    = "RECORD OF PERSONAL HEARING & FINDINGS";
    let order_hdr     = "ORDER";
    let nb1_txt       = "N.B: 1. This copy is granted free of charge for the private use of the person to whom it is issued.";
    let nb2_txt       = "2. An Appeal against this Order shall lie before the Commissioner of Customs (Appeals), Custom House, Chennai-600 001 on payment of 7.5% of the duty demanded where duty or duty and penalty are in dispute, or penalty, where penalty alone is in dispute. The Appeal shall be filed within 60 days provided under Section 128 of the Customs Act, 1962 from the date of receipt of this Order.";
    let note_scn      = "Note: The issue of Show Cause Notice was waived at the instance of the Passenger.";
    let legal_p1      = if is_export {
        "In terms of Foreign Trade Policy notified by the Government in pursuance to Section 3(1) & 3(2) of the Foreign Trade (Development & Regulation) Act, 1992, export of goods without proper Customs declaration or in violation of applicable export regulations / restrictions is prohibited. Passengers are required to declare all goods carried at the time of departure as mandated under Section 40 of the Customs Act, 1962."
    } else {
        "In terms of Foreign Trade Policy notified by the Government in pursuance to Section 3(1) & 3(2) of the Foreign Trade (Development & Regulation) Act, 1992 read with the Rules framed thereunder, also read with Section 11(2)(u) of Customs Act, 1962, import of 'goods in commercial quantity / goods in the nature of non-bonafide baggage' is not permitted without a valid import licence, though exemption exists under clause 3(h) of the Foreign Trade (Exemption from application of Rules in certain cases) order 1993 for import of goods by a passenger from abroad only to the extent admissible under the Baggage Rules framed under Section 79 of the Customs Act, 1962."
    };
    let legal_p2      = if is_export {
        "Export of goods non-declared / misdeclared / concealed / in commercial quantity / contrary to any prohibition or export restriction is therefore liable for confiscation under Section 113 of the Customs Act, 1962 read with Section 3(3) of the Foreign Trade (Development & Regulation) Act, 1992."
    } else {
        "Import of goods non-declared / misdeclared / concealed / in trade and in commercial quantity / non-bonafide in excess of the baggage allowance is therefore liable for confiscation under Section 111(d), (i), (l), (m) & (o) of the Customs Act, 1962 read with Section 3(3) of the Foreign Trade (Development & Regulation) Act, 1992."
    };
    let deputy_sig    = "Deputy / Asst. Commissioner of Customs (Airport)";
    let bottom_nb1    = "N.B: 1. Perishables will be disposed off within seven days from the date of detention.";
    let bottom_nb2    = if !is_export { "2. Where re-export is permitted, the passenger is advised to intimate the date of departure of flight atleast 48 hours in advance." } else { "" };
    let bottom_nb3    = "3. Warehouse rent and Handling Charges are chargeable for the goods detained.";
    let recv_txt      = "Received the Order-in-Original";

    // ── Assemble Typst document ───────────────────────────────────────────────
    let order_paras = {
        let mut p = String::new();
        if !para_rf.is_empty()       { p.push_str(&format!("#par(first-line-indent: 1.5em, justify: true)[{para_rf}]\n#v(2pt)\n")); }
        if !para_ref.is_empty()      { p.push_str(&format!("#par(first-line-indent: 1.5em, justify: true)[{para_ref}]\n#v(2pt)\n")); }
        if !para_abs_conf.is_empty() { p.push_str(&format!("#par(first-line-indent: 1.5em, justify: true)[{para_abs_conf}]\n#v(2pt)\n")); }
        if !para_pp.is_empty()       { p.push_str(&format!("#par(first-line-indent: 1.5em, justify: true)[{para_pp}]\n#v(3pt)\n")); }
        p
    };

    format!(r#"
#set page(width: 8.5in, height: 14in, margin: (top: 0.35in, bottom: 0.3in, left: 0.45in, right: 0.45in))
#set text(font: ("Liberation Sans", "Noto Sans", "Roboto", "Arial"), size: 9pt, lang: "en")
#set par(leading: 0.45em, spacing: 0.6em)
#set table(inset: (x: 3pt, y: 2pt), stroke: 0.75pt + black)

// ═══════════════════════════════ PAGE 1 ═══════════════════════════════════════

// Header
#rect(stroke: 2.5pt + black, inset: 4pt, width: 100%)[
  #align(center)[
    *{office_hdr1}*\
    {office_hdr2}
  ]
]
#v(4pt)
#align(center)[#text(weight: "bold", size: 10pt)[{page1_title}]]
#v(4pt)

// Info table
#table(
  columns: (16%, 18%, 10%, 14%, 18%, 24%),
  [*O.S. No.*],
  [#upper[{oinfo_os_no}/{oinfo_os_year} ({oinfo_booked_by})]],
  [*O.S. Date*], [#upper[{oinfo_os_date}]],
  [*Detention Date*], [#upper[{oinfo_os_date}]],
  table.cell(rowspan: 3)[*Full Name of Passenger\ With Address in India*],
  table.cell(colspan: 3, rowspan: 3)[#upper[{oinfo_pax_name_addr}]],
  [*Passport No. & Date*], [#upper[{oinfo_passport_no} Dt. {oinfo_passport_date}]],
  [*Flight No. & Date*],   [#upper[{oinfo_flight_no} Dt. {oinfo_flight_date}]],
  [*From / To*],           [#upper[{oinfo_from_to}]],
  [*Nationality*], table.cell(colspan: 3)[#upper[{oinfo_nationality}]],
  [*Date of Departure*],   [#upper[{oinfo_date_dep}]],
  [*Duration of Stay Abroad*], table.cell(colspan: 3)[#upper[{oinfo_stay_text}]],
  [*Normal Residence in*], [#upper[{oinfo_residence}]],
  [*Previous Visits, if any*], table.cell(colspan: 5)[#upper[{oinfo_prev_visits}]],
)
#v(4pt)

// Inventory heading
#align(center)[#text(weight: "bold")[{inv_heading}]]
#v(3pt)

// Inventory table
#table(
  columns: (24pt, 1fr, 72pt, 14%, 13%, 18%),
  align: (center, left, center, right, right, right),
  [*S.No.*], [*Description of Goods*], [*Qty.*],
  [*#text(size: 7pt)[{col_fa_hdr}]*],
  [*#text(size: 7.5pt)[{col_duty_hdr}]*\ #v(2pt)#text(weight: "bold")[Value (in Rs.)]],
  [*#text(size: 7pt)[{col_liable_hdr}]*\ Total Value (in Rs.)],
  {rows_markup}
)
#v(4pt)

// Previous offences
#table(
  columns: (auto, 1fr),
  [*Prev. Offence in Above PP No(s). as per COPS*], [*#upper[{oinfo_prev_offence}]*],
  [*Offences of Other PPs(if any)*], [*#upper[{oinfo_other_pp}]*],
)
#v(6pt)

// Summary table
#table(
  columns: (1fr, auto),
  [Value of {col_fa_hdr}],
  [*Rs. {oinfo_total_fa_fmt}{oinfo_fa_dash}*{fa_qty_note}],
  [{sum_duty_txt}], [Rs. {oinfo_total_dutiable_fmt}],
  [*{sum_liab_txt}*], [*Rs. {oinfo_total_items_fmt}*],
)
#v(8pt)

// Signatures row
#grid(columns: (1fr, 1fr),
  [*Name & Signature of Customs Officer*],
  align(right)[*Signature of Passenger*],
)
#v(8pt)

// Remarks
*#underline[Remarks:]* {oinfo_supdts_remarks}

#v(6pt)
#align(right)[*{supdt_sig}*]

#pagebreak()

// ═══════════════════════════════ PAGE 2 ═══════════════════════════════════════

#set text(size: 8pt)
#set par(leading: 0.6em, spacing: 0.8em)

// Office heading
#align(center)[*{p2_office_hdr}*]
#v(2pt)

// Pax + OS row
#grid(columns: (1fr, auto),
  [*Passenger Name:* #upper[{oinfo_pax_name}]],
  [*OS No. {oinfo_os_no}/{oinfo_os_year} ({oinfo_booked_by})  Dated {oinfo_os_date} {day_night}*],
)
#v(3pt)

// Waiver heading
#align(center)[*#underline[{waiver_hdr}]*]
#v(1pt)
#par(first-line-indent: 1.5em, justify: true)[{waiver_txt1}]
#v(2pt)
#align(right)[#v(14pt)_Signature of Passenger_]

#par(first-line-indent: 1.5em, justify: true)[{waiver_txt2}]
#v(2pt)
#align(right)[#v(14pt)_Signature of Passenger_]

// Order passed by
#grid(columns: (1fr, auto),
  [*Order Passed by: Shri. / Smt. / Kum. #upper[{oinfo_adj_name}], {oinfo_adj_desig}*],
  [*Date of Order / Issue: {oinfo_adj_date}*],
)
#v(2pt)

// ORDER (ORIGINAL) heading
#align(center)[*#underline[ORDER (ORIGINAL)]*]
#v(1pt)

// N.B.s
#par(justify: true)[{nb1_txt}]
#par(justify: true)[#h(2em){nb2_txt}]
#v(1pt)

// Note SCN waived
#par(justify: true)[*{note_scn}*]
#v(1pt)

// Legal paragraphs
#par(first-line-indent: 1.5em, justify: true)[{legal_p1}]
#v(1pt)
#par(first-line-indent: 1.5em, justify: true)[{legal_p2}]
#v(2pt)

// Record heading
#align(center)[*#underline[{record_hdr}]*]
#v(1pt)
#par(first-line-indent: 1.5em, justify: true)[{oinfo_adj_remarks}]
#v(2pt)

// ORDER heading
#align(center)[*#underline[{order_hdr}]*]
#v(1pt)

{order_paras}

// Deputy signature
#align(right)[#v(14pt)*{deputy_sig}*]
#v(2pt)

// Bottom N.B.s
#par(justify: true)[{bottom_nb1}]
{bottom_nb2_markup}
#par(justify: true)[#h(2em){bottom_nb3}]
#v(3pt)

// Record table
#table(
  columns: (25%, 23%, 20%, 32%),
  stroke: 0.75pt + black,
  [*B.R.No. And Date*], [#upper[{br_display}]], [*Goods Detained Vide*], [#upper[{dr_display}]],
  [*Duty*], [Rs. ], [*Confiscated goods sent for disposal on*], [],
  [*Redemption / Re-Export Fine*], [Rs. ], [*W.H.No. And Date*], [],
  [*Personal Penalty*], [Rs. ], [], [],
  [*Cash Credit C.No. And Date*], [], [], [],
)
#v(2pt)

// Received + final signatures
#align(right)[*{recv_txt}*]
#v(2pt)
#grid(columns: (1fr, 1fr),
  [#v(14pt)_Signature of the Baggage Officer_],
  align(right)[#v(14pt)_Signature of the Passenger_],
)
"#,
        office_hdr1     = esc(office_hdr1),
        office_hdr2     = esc(office_hdr2),
        page1_title     = esc(page1_title),
        oinfo_os_no     = esc(&os_no),
        oinfo_os_year   = os_year,
        oinfo_booked_by = esc(&booked_by),
        oinfo_os_date   = esc(&os_date),
        oinfo_pax_name_addr = esc(&pax_name_addr),
        oinfo_passport_no   = esc(&passport_no),
        oinfo_passport_date = esc(&passport_date),
        oinfo_flight_no     = esc(&flight_no),
        oinfo_flight_date   = esc(&flight_date),
        oinfo_from_to       = esc(&from_to),
        oinfo_nationality   = esc(&nationality),
        oinfo_date_dep      = esc(&date_of_dep),
        oinfo_stay_text     = esc(&stay_text),
        oinfo_residence     = esc(&residence_at),
        oinfo_prev_visits   = esc(&previous_visits),
        inv_heading         = esc(inv_heading),
        col_fa_hdr          = esc(col_fa_hdr),
        col_duty_hdr        = esc(col_duty_hdr),
        col_liable_hdr      = esc(col_liable_hdr),
        rows_markup         = rows_markup,
        oinfo_prev_offence  = esc(&prev_offence_display),
        oinfo_other_pp      = esc(&other_pp_offences),
        oinfo_total_fa_fmt  = esc(&fmt_indian(total_fa_monetary)),
        oinfo_fa_dash       = if total_fa_monetary > 0.0 { "/-" } else { "" },
        fa_qty_note         = fa_qty_note,
        sum_duty_txt        = esc(sum_duty_txt),
        oinfo_total_dutiable_fmt = esc(&fmt_indian(total_dutiable)),
        sum_liab_txt        = esc(sum_liab_txt),
        oinfo_total_items_fmt    = esc(&fmt_indian(total_items_value)),
        supdt_sig           = esc(supdt_sig),
        oinfo_supdts_remarks= esc(&supdts_remarks),
        p2_office_hdr       = esc(p2_office_hdr),
        oinfo_pax_name      = esc(&pax_name),
        day_night           = day_night,
        waiver_hdr          = esc(waiver_hdr),
        waiver_txt1         = esc(waiver_txt1),
        waiver_txt2         = esc(waiver_txt2),
        oinfo_adj_name      = esc(&adj_name),
        oinfo_adj_desig     = esc(&adj_desig),
        oinfo_adj_date      = esc(&adj_date),
        nb1_txt             = esc(nb1_txt),
        nb2_txt             = esc(nb2_txt),
        note_scn            = esc(note_scn),
        legal_p1            = esc(legal_p1),
        legal_p2            = esc(legal_p2),
        record_hdr          = esc(record_hdr),
        oinfo_adj_remarks   = esc(&adj_remarks),
        order_hdr           = esc(order_hdr),
        order_paras         = order_paras,
        deputy_sig          = esc(deputy_sig),
        bottom_nb1          = esc(bottom_nb1),
        bottom_nb2_markup   = if !bottom_nb2.is_empty() {
            format!("#par(justify: true)[#h(2em){}]", esc(bottom_nb2))
        } else { String::new() },
        bottom_nb3          = esc(bottom_nb3),
        br_display          = br_display,
        dr_display          = dr_display,
        recv_txt            = esc(recv_txt),
    )
}

/// Helper: get the array ref from an empty value (borrow helper).
fn items_ref(v: &Value) -> &Vec<Value> {
    v.as_array().unwrap()
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Public wrapper for `esc()` used by other modules (e.g. backup adjudication PDF).
pub fn esc_pub(s: &str) -> String { esc(s) }

/// Compile arbitrary Typst source text into PDF bytes.
pub fn compile_typst(source_text: &str) -> Result<Vec<u8>> {
    let world  = OsWorld::new(source_text.to_string());
    let result = typst::compile(&world);
    let doc    = result.output
        .map_err(|errors| anyhow::anyhow!("Typst compile error(s): {:?}", errors))?;
    let pdf = typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default())
        .map_err(|e| anyhow::anyhow!("Typst PDF export failed: {e:?}"))?;
    Ok(pdf)
}

/// Generate a 2-page legal-size OS PDF using Typst.
/// `case` is the full cops_master JSON row with an `items` array attached.
pub fn generate_os_pdf(case: &Value) -> Result<Vec<u8>> {
    let source = build_typst_source(case);

    let world  = OsWorld::new(source);
    let result = typst::compile(&world);
    let doc    = result.output
        .map_err(|errors| anyhow::anyhow!("Typst compile error(s): {:?}", errors))?;

    let pdf = typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default())
        .map_err(|e| anyhow::anyhow!("Typst PDF export failed: {e:?}"))?;

    Ok(pdf)
}

/// Generate a BR (Baggage Receipt) PDF.
pub fn generate_br_pdf(case: &Value) -> Result<Vec<u8>> {
    let source = build_br_typst_source(case);
    compile_typst(&source)
}

/// Generate a DR (Detention Receipt) PDF.
pub fn generate_dr_pdf(case: &Value) -> Result<Vec<u8>> {
    let source = build_dr_typst_source(case);
    compile_typst(&source)
}

fn build_br_typst_source(case: &Value) -> String {
    let br_no   = esc(&str_val(case, "br_no"));
    let br_year = i64_val(case, "br_year");
    let br_date = esc(&str_val(case, "br_date"));
    let pax     = esc(&str_val(case, "pax_name"));
    let pp      = esc(&str_val(case, "passport_no"));
    let flight  = esc(&str_val(case, "flight_no"));
    let total_v = fmt_indian(f64_val(case, "total_items_value"));
    let total_d = fmt_indian(f64_val(case, "total_duty_amount"));
    let payable = fmt_indian(f64_val(case, "total_payable"));

    let items_src = case.get("items").and_then(|v| v.as_array())
        .map(|items| {
            items.iter().enumerate().map(|(i, item)| {
                let sno  = i + 1;
                let desc = esc(&str_val(item, "items_desc"));
                let qty  = fmt_qty(f64_val(item, "items_qty"));
                let uqc  = esc(&str_val(item, "items_uqc"));
                let val  = fmt_num(f64_val(item, "items_value"));
                let duty = fmt_num(f64_val(item, "items_duty"));
                format!("  table.cell[{sno}], table.cell[{desc}], table.cell(align:center)[{qty} {uqc}], table.cell(align:right)[{val}], table.cell(align:right)[{duty}],\n")
            }).collect::<String>()
        }).unwrap_or_default();

    format!(r##"
#set page(paper:"a4", margin:(top:1.5cm, bottom:1.5cm, left:2cm, right:1.5cm))
#set text(font:"Libertinus Serif", size:10pt)
#align(center)[
  #text(weight:"bold", size:13pt)[BAGGAGE RECEIPT (BR)]
  #v(0.3em)
  #text(size:11pt)[BR No.: *{br_no}/{br_year}* #h(2cm) Date: *{br_date}*]
]
#v(0.5em)
*Passenger:* {pax} #h(1cm) *Passport No.:* {pp} #h(1cm) *Flight:* {flight}
#v(0.5em)
#table(
  columns: (2em, 1fr, 6em, 5em, 5em),
  align: left,
  table.header[S.No][Description][Qty][Value (Rs.)][Duty (Rs.)],
{items_src})
#v(0.5em)
*Total Value:* Rs.#h(0.3em){total_v} #h(1cm) *Total Duty:* Rs.#h(0.3em){total_d} #h(1cm) *Total Payable:* Rs.#h(0.3em){payable}
"##)
}

fn build_dr_typst_source(case: &Value) -> String {
    let dr_no   = esc(&str_val(case, "dr_no"));
    let dr_year = i64_val(case, "dr_year");
    let dr_date = esc(&str_val(case, "dr_date"));
    let pax     = esc(&str_val(case, "pax_name"));
    let pp      = esc(&str_val(case, "passport_no"));
    let flight  = esc(&str_val(case, "flight_no"));
    let total_v = fmt_indian(f64_val(case, "total_items_value"));

    let items_src = case.get("items").and_then(|v| v.as_array())
        .map(|items| {
            items.iter().enumerate().map(|(i, item)| {
                let sno  = i + 1;
                let desc = esc(&str_val(item, "items_desc"));
                let qty  = fmt_qty(f64_val(item, "items_qty"));
                let uqc  = esc(&str_val(item, "items_uqc"));
                let val  = fmt_num(f64_val(item, "items_value"));
                let cat  = esc(&str_val(item, "items_category"));
                format!("  table.cell[{sno}], table.cell[{desc}], table.cell(align:center)[{qty} {uqc}], table.cell(align:right)[{val}], table.cell[{cat}],\n")
            }).collect::<String>()
        }).unwrap_or_default();

    format!(r##"
#set page(paper:"a4", margin:(top:1.5cm, bottom:1.5cm, left:2cm, right:1.5cm))
#set text(font:"Libertinus Serif", size:10pt)
#align(center)[
  #text(weight:"bold", size:13pt)[DETENTION RECEIPT (DR)]
  #v(0.3em)
  #text(size:11pt)[DR No.: *{dr_no}/{dr_year}* #h(2cm) Date: *{dr_date}*]
]
#v(0.5em)
*Passenger:* {pax} #h(1cm) *Passport No.:* {pp} #h(1cm) *Flight:* {flight}
#v(0.5em)
#table(
  columns: (2em, 1fr, 6em, 5em, 8em),
  align: left,
  table.header[S.No][Description][Qty][Value (Rs.)][Category],
{items_src})
#v(0.5em)
*Total Value of Detained Goods:* Rs.#h(0.3em){total_v}
"##)
}
