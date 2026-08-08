/**
 * Revenue module: duty computation logic.
 *
 * Formulas are NO LONGER hardcoded here.
 * Instead, DrFormulaRule rows (fetched from /dcr/formula-rules) drive all computation.
 * This gives users 100% control over formulas from the Rates & Formulas config page.
 *
 * HOW FORMULA RULES WORK:
 *   1. Rules are evaluated in sort_order (ascending).
 *   2. For each column, the FIRST matching rule wins (IF-THEN-ELSE logic).
 *   3. A rule matches when its condition applies to the current item_desc.
 *   4. expression="" means the column is manual (no auto-computation for that condition).
 *
 * HOW TO ADD A NEW RATE VARIABLE:
 *   1. Add the rate column to DrTariff (backend models/duty_report.py + migration).
 *   2. Add it to the `buildFormulaVars()` return below.
 *   3. Add a seed entry in _seed_duty_report_defaults() (main.py).
 *   4. Users can then reference it in formulas immediately — no code change needed.
 */

export interface DrTariff {
  id: number;
  effective_from: string;
  label: string | null;
  baggage_rate: number;
  liquor_duty_rate: number;
  aidc_liquor_rate: number;
  gold_bcd_rate: number;
  aidc_gold_rate: number;
  gold_cons_bcd_rate: number;
  aidc_gold_cons_rate: number;
  silver_bcd_rate: number;
  aidc_silver_rate: number;
  silver_cons_rate: number;
  aidc_silver_cons_rate: number;
}

export interface DrFormulaRule {
  id: number;
  sort_order: number;
  target_column: string;           // DrEntry field key, e.g. "baggage_duty"
  column_label: string | null;     // human-readable
  condition_type: 'all' | 'only' | 'except';
  condition_items: string;         // comma-separated item names, e.g. "GOLD,SILVER"
  expression: string;              // e.g. "value * baggage_rate"; empty = manual
  is_active: boolean;
  notes: string | null;
}

export interface DrEntry {
  id?: number;
  sort_order: number;
  sl_no: number | null;
  br_no: string;
  os_ref: string;
  item_desc: string;
  dutiable_value: number;
  gold_weight_gms: number;
  baggage_duty: number;
  liquor_duty: number;
  cigarette_duty: number;
  sw_sc: number;
  gold_duty_bcd: number;
  gold_duty_cons: number;
  silver_duty_cons: number;
  sws_on_gold: number;
  aidc_gold_silver: number;
  sws_on_silver: number;
  aidc_on_liquor: number;
  redemption_fine: number;
  reexport_fine: number;
  personal_penalty: number;
  other_charges: number;
  fuel_duty: number;
  total_duty: number;
  flight_no: string;
  is_sbi_challan: boolean;
  is_offline_br: boolean;
  overrides: string | null;
}

export interface DrDrEntry {
  id?: number;
  sort_order: number;
  dr_no: string;
  amount: number;
  item_desc: string;
  remarks: string;
}

export interface DrOsEntry {
  id?: number;
  sort_order: number;
  os_no: string;
  amount: number;
  item_desc: string;
  remarks: string;
}

export interface DrSession {
  id: number;
  report_date: string;
  shift: 'DAY' | 'NIGHT';
  batch_name: string | null;
  tariff_id: number | null;
  created_by: string | null;
  created_at: string | null;
  submitted_at: string | null;
  submitted_by: string | null;
  tariff: DrTariff | null;
  entries: DrEntry[];
  dr_entries: DrDrEntry[];
  os_entries: DrOsEntry[];
}

// ── Formula variable names (available in expression strings) ─────────────────

/** All variables that can be used in a formula expression. */
export const FORMULA_VARS: { name: string; label: string }[] = [
  { name: 'value',               label: 'Dutiable Value' },
  { name: 'weight',              label: 'Gold Weight (grams)' },
  { name: 'baggage_rate',        label: 'Baggage Rate' },
  { name: 'liquor_duty_rate',    label: 'Liquor Duty Rate' },
  { name: 'aidc_liquor_rate',    label: 'AIDC on Liquor Rate' },
  { name: 'gold_bcd_rate',       label: 'Gold BCD Rate' },
  { name: 'aidc_gold_rate',      label: 'AIDC Gold/Silver Rate' },
  { name: 'gold_cons_bcd_rate',  label: 'Gold Concessional BCD Rate' },
  { name: 'aidc_gold_cons_rate', label: 'AIDC Gold(C) Rate' },
  { name: 'silver_bcd_rate',     label: 'Silver BCD Rate' },
  { name: 'aidc_silver_rate',    label: 'AIDC Silver Rate' },
  { name: 'silver_cons_rate',    label: 'Silver Concessional Rate' },
  { name: 'aidc_silver_cons_rate', label: 'AIDC Silver(C) Rate' },
];

/** Build the variable map for formula evaluation from the current tariff + entry values. */
export function buildFormulaVars(
  tariff: DrTariff,
  dutiableValue: number,
  goldWeightGms: number,
): Record<string, number> {
  return {
    value:                 dutiableValue  || 0,
    weight:                goldWeightGms  || 0,
    baggage_rate:          tariff.baggage_rate,
    liquor_duty_rate:      tariff.liquor_duty_rate,
    aidc_liquor_rate:      tariff.aidc_liquor_rate,
    gold_bcd_rate:         tariff.gold_bcd_rate,
    aidc_gold_rate:        tariff.aidc_gold_rate,
    gold_cons_bcd_rate:    tariff.gold_cons_bcd_rate,
    aidc_gold_cons_rate:   tariff.aidc_gold_cons_rate,
    silver_bcd_rate:       tariff.silver_bcd_rate,
    aidc_silver_rate:      tariff.aidc_silver_rate,
    silver_cons_rate:      tariff.silver_cons_rate,
    aidc_silver_cons_rate: tariff.aidc_silver_cons_rate,
  };
}

/**
 * Evaluate a formula expression — THROWS on any error.
 * Only arithmetic operators (+, -, *, /) and known variable names are allowed.
 * Callers that want a silent fallback should catch the exception themselves.
 */
export function evalFormula(
  expr: string,
  vars: Record<string, number>,
): number {
  if (!expr.trim()) return 0;
  // Replace known variable names with their numeric values
  const substituted = expr.trim().replace(/[a-zA-Z_][a-zA-Z0-9_]*/g, (name) => {
    if (name in vars) return String(vars[name]);
    throw new Error(`Unknown variable: ${name}`);
  });
  // After substitution, only digits, operators, dots, parens, spaces should remain
  if (!/^[\d\s+\-*/.()]+$/.test(substituted)) {
    throw new Error('Invalid expression after substitution');
  }
  // eslint-disable-next-line no-new-func
  const result = Number(Function('"use strict"; return (' + substituted + ')')());
  if (!isFinite(result)) throw new Error('Expression produced non-finite result (division by zero?)');
  return result;
}

/**
 * Like evalFormula but silently returns 0 on any error.
 * Used inside computeDuties where bad expressions should not abort the whole row.
 */
function evalFormulaSafe(expr: string, vars: Record<string, number>): number {
  try {
    return evalFormula(expr, vars);
  } catch {
    return 0;
  }
}

/** Validate a formula expression and return an error string (or null if valid). */
export function validateExpression(
  expr: string,
  sampleVars?: Record<string, number>,
): string | null {
  if (!expr.trim()) return null; // empty = manual, always valid
  const vars = sampleVars ?? Object.fromEntries(FORMULA_VARS.map(v => [v.name, 1]));
  try {
    evalFormula(expr, vars);
    return null;
  } catch (e) {
    return e instanceof Error ? e.message : 'Invalid expression';
  }
}

// ── Rule matching ─────────────────────────────────────────────────────────────

function ruleMatchesItem(rule: DrFormulaRule, itemDesc: string): boolean {
  const desc = itemDesc.toUpperCase().trim();
  const items = rule.condition_items
    .split(',')
    .map(s => s.trim().toUpperCase())
    .filter(Boolean);

  switch (rule.condition_type) {
    case 'all':    return true;
    case 'only':   return items.includes(desc);
    case 'except': return !items.includes(desc);
    default:       return false;
  }
}

// ── Core computation ──────────────────────────────────────────────────────────

/**
 * Returns the set of column names that are auto-computed (not manual) for the
 * given item description, using the current formula rules.
 * Used by the spreadsheet to shade auto-computed cells sky-blue.
 */
export function getAutoFields(
  rules: DrFormulaRule[],
  itemDesc: string,
): Set<string> {
  if (!itemDesc.trim()) return new Set();
  const auto = new Set<string>();
  const seen = new Set<string>(); // first-match-wins per column

  for (const rule of rules) {
    if (!rule.is_active) continue;
    if (seen.has(rule.target_column)) continue;
    if (ruleMatchesItem(rule, itemDesc)) {
      seen.add(rule.target_column);
      if (rule.expression.trim()) {
        // Has a formula → this column is auto-computed for this item
        auto.add(rule.target_column);
      }
      // No expression → column is manual for this item (not added to auto set)
    }
  }
  return auto;
}

/**
 * Compute all auto-computed duty fields from formula rules + current tariff.
 * Returns only the fields that should be updated; the caller merges into the full entry.
 * Manual columns (no expression) are set to 0 in the result (not touched if overridden).
 *
 * NOTE: Existing DrEntry rows in the DB are NEVER recomputed from new rules.
 * Only the frontend calls this for entries being actively edited.
 */
export function computeDuties(
  tariff: DrTariff,
  rules: DrFormulaRule[],
  itemDesc: string,
  dutiableValue: number,
  goldWeightGms: number = 0,
): Partial<DrEntry> {
  if (!itemDesc.trim()) return {};
  const vars = buildFormulaVars(tariff, dutiableValue, goldWeightGms);
  const result: Partial<DrEntry> = {};
  const seen = new Set<string>();

  for (const rule of rules) {
    if (!rule.is_active) continue;
    const col = rule.target_column;
    if (seen.has(col)) continue;
    if (!ruleMatchesItem(rule, itemDesc)) continue;
    seen.add(col);

    if (rule.expression.trim()) {
      (result as Record<string, number>)[col] = round2(evalFormulaSafe(rule.expression, vars));
    } else {
      // Manual column for this item — reset to 0 only when item changes
      // (don't touch if user already entered a value; caller handles overrides)
      (result as Record<string, number>)[col] = 0;
    }
  }

  return result;
}

/** Compute total from all duty column values (mirrors Excel ROUND(SUM(G:V),0)) */
export function computeTotal(entry: Partial<DrEntry>): number {
  const fields: (keyof DrEntry)[] = [
    'baggage_duty', 'liquor_duty', 'cigarette_duty', 'sw_sc',
    'gold_duty_bcd', 'gold_duty_cons', 'silver_duty_cons',
    'sws_on_gold', 'aidc_gold_silver', 'sws_on_silver', 'aidc_on_liquor',
    'redemption_fine', 'reexport_fine', 'personal_penalty',
    'other_charges', 'fuel_duty',
  ];
  const sum = fields.reduce((acc, f) => acc + ((entry[f] as number) || 0), 0);
  return Math.round(sum);
}

function round2(n: number): number {
  return Math.round(n * 100) / 100;
}

// ── Levenshtein / fuzzy search ────────────────────────────────────────────────

export function levenshtein(a: string, b: string): number {
  const la = a.toLowerCase();
  const lb = b.toLowerCase();
  if (la === lb) return 0;
  const m = la.length, n = lb.length;
  const dp: number[][] = Array.from({ length: m + 1 }, (_, i) => [i]);
  for (let j = 0; j <= n; j++) dp[0][j] = j;
  for (let i = 1; i <= m; i++)
    for (let j = 1; j <= n; j++)
      dp[i][j] = la[i - 1] === lb[j - 1]
        ? dp[i - 1][j - 1]
        : 1 + Math.min(dp[i - 1][j], dp[i][j - 1], dp[i - 1][j - 1]);
  return dp[m][n];
}

export function fuzzyMatch<T extends { id: number; name: string; usage_count: number }>(
  query: string,
  items: T[],
  maxResults = 8,
): (T & { score: number })[] {
  const q = query.toLowerCase().trim();
  if (!q) return items.slice(0, maxResults).map(i => ({ ...i, score: 0 }));

  return items
    .map(item => {
      const name = item.name.toLowerCase();
      const exact = name === q ? -2 : 0;
      const starts = name.startsWith(q) ? -1 : 0;
      const dist = levenshtein(q, name);
      return { ...item, score: exact + starts + dist };
    })
    .filter(i => i.score < i.name.length)
    .sort((a, b) => a.score - b.score || b.usage_count - a.usage_count)
    .slice(0, maxResults);
}

// ── Formatting helpers ────────────────────────────────────────────────────────

export function fmtDate(d: Date | string): string {
  const dt = typeof d === 'string' ? new Date(d + 'T00:00:00') : d;
  return dt.toLocaleDateString('en-GB').replace(/\//g, '.');
}

// ── WhatsApp message builder ──────────────────────────────────────────────────

export function buildMessage(
  session: DrSession,
  settings: { greeting_recipients: string },
): string {
  const hour = new Date().getHours();
  const greeting =
    hour < 12 ? 'Good Morning' :
    hour < 17 ? 'Good Afternoon' :
                'Good Evening';

  const dateStr = fmtDate(session.report_date);
  const shiftLabel = session.shift === 'DAY' ? 'Day Shift' : 'Night Shift';
  const batchName = session.batch_name || 'Batch';

  const itemTotals = new Map<string, number>();
  for (const e of session.entries) {
    if (e.item_desc) {
      itemTotals.set(e.item_desc, (itemTotals.get(e.item_desc) || 0) + (e.total_duty || 0));
    }
  }

  const totalDuty = session.entries.reduce((s, e) => s + (e.total_duty || 0), 0);
  const drTotal = session.dr_entries.reduce((s, e) => s + (e.amount || 0), 0);
  const osTotal = session.os_entries.reduce((s, e) => s + (e.amount || 0), 0);

  const lines = [
    `${greeting} ${settings.greeting_recipients},`,
    ``,
    `*Duty Collection Report - ${dateStr} (${shiftLabel}) - ${batchName}*`,
    ``,
  ];

  for (const [item, total] of itemTotals.entries()) {
    lines.push(`• ${item}: ₹${total.toLocaleString('en-IN')}`);
  }

  if (itemTotals.size > 0) lines.push('');
  lines.push(`*Total Duty Collected: ₹${Math.round(totalDuty).toLocaleString('en-IN')}*`);

  if (drTotal > 0) lines.push(`DR Total: ₹${Math.round(drTotal).toLocaleString('en-IN')}`);
  if (osTotal > 0) lines.push(`OS Total: ₹${Math.round(osTotal).toLocaleString('en-IN')}`);

  return lines.join('\n');
}

// ── Blank row factories ───────────────────────────────────────────────────────

export function blankEntry(sortOrder: number): DrEntry {
  return {
    sort_order: sortOrder,
    sl_no: null,
    br_no: '',
    os_ref: '',
    item_desc: '',
    dutiable_value: 0,
    gold_weight_gms: 0,
    baggage_duty: 0,
    liquor_duty: 0,
    cigarette_duty: 0,
    sw_sc: 0,
    gold_duty_bcd: 0,
    gold_duty_cons: 0,
    silver_duty_cons: 0,
    sws_on_gold: 0,
    aidc_gold_silver: 0,
    sws_on_silver: 0,
    aidc_on_liquor: 0,
    redemption_fine: 0,
    reexport_fine: 0,
    personal_penalty: 0,
    other_charges: 0,
    fuel_duty: 0,
    total_duty: 0,
    flight_no: '',
    is_sbi_challan: false,
    is_offline_br: false,
    overrides: null,
  };
}

export function blankDrEntry(sortOrder: number): DrDrEntry {
  return { sort_order: sortOrder, dr_no: '', amount: 0, item_desc: '', remarks: '' };
}

export function blankOsEntry(sortOrder: number): DrOsEntry {
  return { sort_order: sortOrder, os_no: '', amount: 0, item_desc: '', remarks: '' };
}
