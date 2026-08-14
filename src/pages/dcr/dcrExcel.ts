/**
 * Client-side Excel generation for DCR reports using SheetJS.
 * COPS2 generates Excel on the frontend (no server-side openpyxl needed).
 * Data is taken directly from the React state — faster, no extra round-trip.
 */
import type { DrEntry, DrSession, DrDrEntry, DrOsEntry } from './revenueCalc';
import { fmtDate } from './revenueCalc';
import { showDownloadToast } from '@/components/DownloadToast';

// Type-only — erased at build time, so importing it costs nothing at runtime.
import type * as XLSXTypes from 'xlsx-js-style';

/**
 * The spreadsheet library is loaded ON DEMAND, not when this module is
 * imported. It is by far the largest dependency in the app, and an officer who
 * never exports a sheet should never pay to load it — opening the duty register
 * used to fetch it whether or not anyone pressed a download button.
 */
let _xlsx: typeof XLSXTypes | null = null;
async function xlsx(): Promise<typeof XLSXTypes> {
  if (!_xlsx) _xlsx = await import('xlsx-js-style');
  return _xlsx;
}

// ── Column definitions ────────────────────────────────────────────────────────

/**
 * The daily revenue report, as the office has always drawn it.
 *
 * Columns A–Y are the report the customs house has used for years, in its own
 * order and wording, so a shift's sheet reads the same whoever produced it. The
 * SBI marker is added at the end rather than inserted among them: it is ours,
 * not part of the standard form, and putting it in the middle would shift every
 * column after it.
 *
 * The headers used to run one short of the values written beneath them — there
 * was no column for the cess on cigarettes, though every row carried one — so
 * from "Total Duty" rightwards each figure sat under the wrong heading, and the
 * duty column showed the cess. The two lists are now built from one another and
 * cannot drift apart again.
 */
const REVENUE_HEADERS = [
  'SR. NO.', 'BR NO. AND BR TYPE', 'OS No.',
  'Item Description', 'Total Dutiable Value',
  'GOLD Weight(gms)', 'Baggage duty', 'Liquor Duty',
  'Cigarettes Duty', 'SW SC on Bagg/Lqr/Cig',
  'GOLD DUTY (BCD)', 'Gold Duty (Cons.Rate BCD)',
  'Silv.Duty (Cons.Rate)', 'SWS on GOLD',
  'AIDC on Gold/SILVER', 'SWS ON SILVER',
  'Aidc on Liqr', 'Redemption Fine', 'Re-Export Fine',
  'Personal Penalty', 'OTHER Charges', 'FUEL DUTY',
  'CESS on CIG', 'Total Duty', 'Flight No', 'Offline (SBI)',
];

/** One field per heading above, in the same order. */
const REVENUE_FIELDS: (keyof DrEntry | 'sr_no')[] = [
  'sr_no', 'br_no', 'os_ref',
  'item_desc', 'dutiable_value',
  'gold_weight_gms', 'baggage_duty', 'liquor_duty',
  'cigarette_duty', 'sw_sc',
  'gold_duty_bcd', 'gold_duty_cons',
  'silver_duty_cons', 'sws_on_gold',
  'aidc_gold_silver', 'sws_on_silver',
  'aidc_on_liquor', 'redemption_fine', 'reexport_fine',
  'personal_penalty', 'other_charges', 'fuel_duty',
  'cess_on_cig', 'total_duty', 'flight_no', 'is_offline_br',
];

/** The money columns, and the gold weight, which the TOTAL row adds up. */
const REVENUE_SUMMED = new Set<string>([
  'dutiable_value', 'gold_weight_gms',
  'baggage_duty', 'liquor_duty', 'cigarette_duty', 'sw_sc',
  'gold_duty_bcd', 'gold_duty_cons', 'silver_duty_cons',
  'sws_on_gold', 'aidc_gold_silver', 'sws_on_silver', 'aidc_on_liquor',
  'redemption_fine', 'reexport_fine', 'personal_penalty',
  'other_charges', 'fuel_duty', 'cess_on_cig', 'total_duty',
]);

/** The consolidated sheet, head by head, in the order the office reads them. */
const DUTY_HEADS: [string, keyof DrEntry][] = [
  ['Baggage duty',                'baggage_duty'],
  ['Liquor Duty',                 'liquor_duty'],
  ['Cigarettes Duty',             'cigarette_duty'],
  ['CESS on Cigarettes',          'cess_on_cig'],
  ['SW SC on Bagg/Lqr/Cig',       'sw_sc'],
  ['Gold Duty (BCD)',             'gold_duty_bcd'],
  ['Gold Duty (Cons.Rate BCD)',   'gold_duty_cons'],
  ['SWS on Gold Duty',            'sws_on_gold'],
  ['Silv.Duty (Cons.Rate)',       'silver_duty_cons'],
  ['AIDC On Silv/Gold',           'aidc_gold_silver'],
  ['SWS On Silver',               'sws_on_silver'],
  ['Aidc on Liqr/Others',         'aidc_on_liquor'],
  ['Redemption Fine',             'redemption_fine'],
  ['Re-Export Fine',              'reexport_fine'],
  ['Personal Penalty/Bail',       'personal_penalty'],
  ['Misc./Other Charges',         'other_charges'],
  ['FUEL',                        'fuel_duty'],
];

/**
 * Give each column the width its contents actually need.
 *
 * The widths used to be a fixed list, so a long item description was cut off
 * while a column of five-digit receipt numbers sat half empty. This measures
 * what is in the column and sizes it to that, within limits — nothing narrower
 * than is readable, nothing so wide the sheet runs off the page. Headings wrap,
 * so a column need only be as wide as the longest single word in its heading.
 */
function fitColumns(rows: unknown[][], headerRow = 0, minW = 7, maxW = 26) {
  const widest: number[] = [];
  rows.forEach((row, ri) => {
    (row || []).forEach((v, ci) => {
      if (v === null || v === undefined || v === '') return;
      const text = typeof v === 'number' ? Math.round(v).toLocaleString('en-IN') : String(v);
      const len = ri === headerRow
        ? Math.max(...text.split(/\s+/).map(w => w.length))   // headings may wrap
        : Math.max(...text.split('\n').map(l => l.length));
      widest[ci] = Math.max(widest[ci] ?? 0, len);
    });
  });
  const cols = Math.max(rows[headerRow]?.length ?? 0, widest.length);
  return Array.from({ length: cols },
    (_, i) => ({ wch: Math.max(minW, Math.min(maxW, (widest[i] ?? 0) + 2)) }));
}

/** Let the long columns wrap instead of running under their neighbour. */
function wrapColumns(XLSX: typeof XLSXTypes, ws: XLSXTypes.WorkSheet, columns: number[]) {
  const range = XLSX.utils.decode_range(ws['!ref'] ?? 'A1');
  for (let r = range.s.r; r <= range.e.r; r++) {
    for (const c of columns) {
      const cell = ws[XLSX.utils.encode_cell({ r, c })] as XLSXTypes.CellObject | undefined;
      if (!cell) continue;
      cell.s = { ...(cell.s ?? {}), alignment: { wrapText: true, vertical: 'center' } };
    }
  }
}

const ADC_HEADERS = [
  'SR. NO.', 'BR NO. AND BR TYPE', 'Item Description',
  'Total Dutiable Value', 'Total Duty', 'Flight No',
];

// ── Helpers ───────────────────────────────────────────────────────────────────

function triggerDownload(buf: ArrayBuffer, fname: string) {
  const blob = new Blob([buf], {
    type: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
  });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = fname;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
  showDownloadToast(fname);
}

const num = (v: unknown): number => (typeof v === 'number' ? v : 0);

/**
 * One receipt's row, in the order the headings run.
 *
 * A receipt covering three items is written as three rows. The serial and the
 * receipt number belong to the receipt rather than to each item, so they are
 * written once and merged down the group — the way the register is kept by hand.
 */
function entryToRevenueRow(e: DrEntry, srNo: number | null, isSubRow: boolean): unknown[] {
  return REVENUE_FIELDS.map(f => {
    if (f === 'sr_no') return isSubRow ? null : srNo;
    if (f === 'br_no') return isSubRow ? null : (e.br_no || '');
    if (f === 'is_offline_br') return e.is_offline_br ? 'SBI' : '';
    const v = e[f as keyof DrEntry];
    if (typeof v === 'number') return v || null;
    return (v as string) || '';
  });
}

function makeTotalRow(entries: DrEntry[]): unknown[] {
  return REVENUE_FIELDS.map(f => {
    if (f === 'item_desc') return 'TOTAL';
    if (!REVENUE_SUMMED.has(f as string)) return '';
    return Math.round(entries.reduce((t, e) => t + num(e[f as keyof DrEntry]), 0)) || null;
  });
}

/**
 * The item table that sits under the receipts.
 *
 * It has to come to the same figure as the receipts above it, so it takes in
 * every row that carries money — including one where nobody typed an item, which
 * would otherwise be dropped and leave the table quietly short. Items are
 * whatever the officers type and a new one appears the day it is first used; two
 * spellings of the same thing are folded together, and the first spelling used is
 * the one shown. It sums each row's own total, so a total corrected by hand is
 * carried here too.
 */
function itemSummary(entries: DrEntry[]): { name: string; count: number; total: number }[] {
  const UNNAMED = 'NOT SPECIFIED';
  const out = new Map<string, { name: string; count: number; total: number }>();
  for (const e of entries) {
    const amount = num(e.total_duty);
    const raw = (e.item_desc || '').trim();
    if (!raw && !amount) continue;                        // an empty row of the sheet
    const key = raw.replace(/\s+/g, ' ').toUpperCase() || UNNAMED;
    const row = out.get(key) ?? { name: raw || UNNAMED, count: 0, total: 0 };
    row.count += 1;
    row.total += amount;
    out.set(key, row);
  }
  return [...out.values()];
}

/** Where the sub-tables sit, in columns, under the receipts. */
const SUB_ITEM = 1, SUB_DR = 7, SUB_OS = 12;

function makeRevenueSheet(
  XLSX: typeof XLSXTypes,
  entries: DrEntry[],
  drEntries: DrDrEntry[] = [],
  osEntries: DrOsEntry[] = [],
): XLSXTypes.WorkSheet {
  const rows: unknown[][] = [REVENUE_HEADERS];
  const merges: XLSXTypes.Range[] = [];

  let srNo = 0, runStart = 1;
  entries.forEach((e, i) => {
    const prev = i > 0 ? entries[i - 1] : null;
    const isSubRow = !!(e.br_no && prev && e.br_no === prev.br_no);
    if (!isSubRow) {
      if (i > 0 && rows.length - 1 > runStart) {
        // close the group that just ended: club its serial and receipt number
        for (const c of [0, 1]) merges.push({ s: { r: runStart, c }, e: { r: rows.length - 1, c } });
      }
      runStart = rows.length;
      srNo += 1;
    }
    rows.push(entryToRevenueRow(e, srNo, isSubRow));
  });
  if (entries.length && rows.length - 1 > runStart) {
    for (const c of [0, 1]) merges.push({ s: { r: runStart, c }, e: { r: rows.length - 1, c } });
  }

  rows.push(makeTotalRow(entries));

  // The three sub-tables, side by side, three rows below the receipts.
  rows.push([], []);
  const items = itemSummary(entries);
  const head: unknown[] = [];
  head[SUB_ITEM] = 'ITEM'; head[SUB_ITEM + 2] = 'NO OF BR'; head[SUB_ITEM + 3] = 'TOTAL DUTY';
  head[SUB_DR] = 'DR'; head[SUB_DR + 1] = 'AMOUNT (IN Rs.)'; head[SUB_DR + 2] = 'ITEM'; head[SUB_DR + 3] = 'REMARKS';
  head[SUB_OS] = 'OS'; head[SUB_OS + 1] = 'AMOUNT'; head[SUB_OS + 2] = 'ITEM'; head[SUB_OS + 3] = 'REMARKS';
  rows.push(head);

  const depth = Math.max(items.length, drEntries.length, osEntries.length);
  for (let i = 0; i < depth; i++) {
    const r: unknown[] = [];
    const it = items[i];
    if (it) { r[SUB_ITEM] = it.name; r[SUB_ITEM + 2] = it.count; r[SUB_ITEM + 3] = Math.round(it.total); }
    const dr = drEntries[i];
    if (dr) { r[SUB_DR] = dr.dr_no; r[SUB_DR + 1] = dr.amount; r[SUB_DR + 2] = dr.item_desc; r[SUB_DR + 3] = dr.remarks; }
    const os = osEntries[i];
    if (os) { r[SUB_OS] = os.os_no; r[SUB_OS + 1] = os.amount; r[SUB_OS + 2] = os.item_desc; r[SUB_OS + 3] = os.remarks; }
    rows.push(r);
  }

  const foot: unknown[] = [];
  foot[SUB_ITEM] = 'TOTAL';
  foot[SUB_ITEM + 2] = items.reduce((t, i) => t + i.count, 0) || null;
  foot[SUB_ITEM + 3] = Math.round(items.reduce((t, i) => t + i.total, 0)) || null;
  foot[SUB_DR] = 'TOTAL';
  foot[SUB_DR + 1] = Math.round(drEntries.reduce((t, d) => t + num(d.amount), 0)) || null;
  foot[SUB_OS] = 'TOTAL';
  foot[SUB_OS + 1] = Math.round(osEntries.reduce((t, o) => t + num(o.amount), 0)) || null;
  rows.push(foot);

  const ws = XLSX.utils.aoa_to_sheet(rows);
  ws['!cols'] = fitColumns(rows);
  // the item description, and the item name in the table below it
  wrapColumns(XLSX, ws, [3, SUB_ITEM]);
  if (merges.length) ws['!merges'] = merges;
  return ws;
}

/**
 * The consolidated sheet: what the shift collected, head by head.
 *
 * The DAY and NIGHT columns must come to what the DAY and NIGHT sheets say — an
 * officer signs all three, and they are the same money. The heads are built from
 * the individual columns, so on the rare occasion a row's total was corrected by
 * hand they cannot show it on their own; the difference is carried as a row of
 * its own, named. Every head stays true to its column, the column adds up to the
 * total, and nothing is hidden inside a head that never received it. That row is
 * written only when there is a difference.
 */
function makeConsolidatedSheet(
  XLSX: typeof XLSXTypes,
  dayEntries: DrEntry[],
  nightEntries: DrEntry[],
): XLSXTypes.WorkSheet {
  const headSum = (es: DrEntry[], f: keyof DrEntry) =>
    Math.round(es.reduce((t, e) => t + num(e[f]), 0));
  const correction = (es: DrEntry[]) =>
    Math.round(es.reduce((t, e) => t + num(e.total_duty), 0))
    - DUTY_HEADS.reduce((t, [, f]) => t + headSum(es, f), 0);

  const rows: unknown[][] = [
    ['Revenue Report — Consolidated (Day & Night Shifts)'],
    ['SR', 'Description', 'DAY (₹)', 'NIGHT (₹)', 'TOTAL (₹)'],
  ];
  const heads: [string, number, number][] =
    DUTY_HEADS.map(([label, f]) => [label, headSum(dayEntries, f), headSum(nightEntries, f)]);

  const dayFix = correction(dayEntries), nightFix = correction(nightEntries);
  if (dayFix || nightFix) heads.push(['Correction to totals entered by hand', dayFix, nightFix]);

  heads.forEach(([label, d, n], i) =>
    rows.push([i + 1, label, d || null, n || null, (d + n) || null]));

  const td = heads.reduce((t, [, d]) => t + d, 0);
  const tn = heads.reduce((t, [, , n]) => t + n, 0);
  rows.push(['', 'TOTAL DUTY', td || null, tn || null, (td + tn) || null]);

  const ws = XLSX.utils.aoa_to_sheet(rows);
  ws['!cols'] = fitColumns(rows, 1, 6, 36);
  wrapColumns(XLSX, ws, [1]);                       // the head descriptions
  ws['!merges'] = [{ s: { r: 0, c: 0 }, e: { r: 0, c: 4 } }];
  return ws;
}

// ── Public API ────────────────────────────────────────────────────────────────

/**
 * Generate and download the ADC (Arrival Duty Collection) report Excel for one session.
 * Called from RevenueSheet when the user clicks "ADC" download.
 */
export async function downloadAdcExcel(session: DrSession, entries: DrEntry[]): Promise<void> {
  const XLSX = await xlsx();
  const shift = session.shift === 'DAY' ? 'D' : 'N';
  const fname = `${fmtDate(session.report_date)}(${shift}) ADC REPORT.xlsx`;

  const rows = entries.map((e, i) => [
    i + 1,
    e.br_no || '',
    e.item_desc || '',
    e.dutiable_value || 0,
    e.total_duty || 0,
    e.flight_no || '',
  ]);

  // Totals row
  const totalDutiable = entries.reduce((s, e) => s + (e.dutiable_value || 0), 0);
  const totalDuty     = entries.reduce((s, e) => s + (e.total_duty     || 0), 0);
  rows.push(['', 'TOTAL', '', totalDutiable, totalDuty, '']);

  const wb = XLSX.utils.book_new();
  const ws = XLSX.utils.aoa_to_sheet([ADC_HEADERS, ...rows]);
  ws['!cols'] = [{ wch: 6 }, { wch: 22 }, { wch: 26 }, { wch: 18 }, { wch: 15 }, { wch: 12 }];
  XLSX.utils.book_append_sheet(wb, ws, 'ADC Report');

  const buf = XLSX.write(wb, { type: 'array', bookType: 'xlsx' });
  triggerDownload(buf, fname);
}

/**
 * Generate and download the combined Revenue Report Excel for a date.
 * Includes day-shift sheet, night-shift sheet, and a consolidated sheet.
 *
 * Call this after fetching full session data (with entries) for both shifts.
 * Pass null for a shift that doesn't exist.
 */
export async function downloadRevenueExcel(
  reportDate: string,
  daySession: DrSession | null,
  nightSession: DrSession | null,
): Promise<void> {
  const XLSX = await xlsx();
  const fname = `${fmtDate(reportDate)} REVENUE REPORT.xlsx`;
  const wb = XLSX.utils.book_new();

  const dayEntries   = daySession?.entries   ?? [];
  const nightEntries = nightSession?.entries ?? [];

  // Both shifts are always written, whether or not the second one has started —
  // the report is a form with three sheets, and a missing sheet reads as a
  // missing page rather than as a quiet shift.
  XLSX.utils.book_append_sheet(
    wb, makeRevenueSheet(XLSX, dayEntries, daySession?.dr_entries ?? [], daySession?.os_entries ?? []),
    'DAY');
  XLSX.utils.book_append_sheet(
    wb, makeRevenueSheet(XLSX, nightEntries, nightSession?.dr_entries ?? [], nightSession?.os_entries ?? []),
    'NIGHT');
  XLSX.utils.book_append_sheet(
    wb, makeConsolidatedSheet(XLSX, dayEntries, nightEntries), 'CONSOLIDATED');

  const buf = XLSX.write(wb, { type: 'array', bookType: 'xlsx' });
  triggerDownload(buf, fname);
}

// ═══════════════════════════════════════════════════════════════════════════
// Monthly revenue register
//
// Ported column-for-column from OS_module_upgrade's _build_monthly_revenue_sheet.
// This is the month-end filing, so the layout is not ours to improve: 31 columns
// in a fixed order, one block per session, and a grand total the office reconciles
// against the bank. Figures in the wrong column are worse than no report at all.
//
// The colour coding carries meaning and is not decoration:
//   BLUE bold BR number  = collected online through the portal — NOT covered by
//                          the session's bank challan
//   BLACK BR number      = offline collection, covered by the challan
// The offline/online split is also written out in the two subtotal rows beneath
// each session, so the distinction survives even in black and white.
// ═══════════════════════════════════════════════════════════════════════════

const MONTHLY_HEADERS = [
  'Sl. No.', 'Challan No.', 'Date',
  'BR No.', 'Item Description', 'Quantity OF Gold/ Silver (in Gms)',
  'Total Value\nBaggage', 'Total Value\nGold /Silver',
  'Baggage duty', 'Liquor Duty', 'Cig Duty',
  'SW SC on Bagg / Lqr / Cig',
  'Gold Duty (35%)', 'Gold Duty (Cons.Rate)',
  'Silver Duty (35%)', 'Silver Duty (Cons.Rate)',
  'SW SC on Gold Duty', 'AIDC on Gold',
  'SW SC on Silver Duty', 'AIDC on Silver Duty',
  'AIDC on Liqr', 'RF', 'Re-Export Fine',
  'Personal Penalty', 'MISC. CHARGES',
  'CESS on CIG', 'FUEL DUTY',
  'TOTAL',
  'DATES', 'FORT 1/2', 'REMARKS',
];

const MONTHLY_WIDTHS = [5, 10, 12, 10, 24, 8, 10, 10, 10, 10, 8, 10,
                        10, 10, 10, 10, 10, 10, 10, 10, 10, 8, 10, 10,
                        10, 8, 10, 10, 10, 8, 12];

const GOLD_ITEMS   = new Set(['GOLD', 'GOLD(C)']);
const SILVER_ITEMS = new Set(['SILVER', 'SILVER(C)']);

const THIN = { style: 'thin', color: { rgb: '000000' } } as const;
const BORDER = { top: THIN, bottom: THIN, left: THIN, right: THIN };

/** Cell style helper — right-aligns the money columns, as the register does. */
function cellStyle(col: number, opts: {
  bold?: boolean; size?: number; fill?: string; color?: string; italic?: boolean;
} = {}) {
  return {
    font: {
      sz: opts.size ?? 8,
      bold: opts.bold ?? false,
      italic: opts.italic ?? false,
      color: { rgb: opts.color ?? '000000' },
    },
    fill: opts.fill ? { patternType: 'solid', fgColor: { rgb: opts.fill } } : undefined,
    border: BORDER,
    alignment: { vertical: 'center', horizontal: col >= 5 ? 'right' : 'left' },
  };
}

/**
 * Build and download the month's revenue register.
 * `sessions` must already carry their entries and os_entries.
 */
export async function downloadMonthlyRevenueExcel(
  year: number,
  month: number,
  sessions: DrSession[],
): Promise<void> {
  const XLSX = await xlsx();

  const rows: unknown[][] = [MONTHLY_HEADERS];
  const merges: XLSXTypes.Range[] = [];
  const styles = new Map<string, ReturnType<typeof cellStyle>>();
  const setStyle = (r: number, c: number, st: ReturnType<typeof cellStyle>) =>
    styles.set(XLSX.utils.encode_cell({ r, c }), st);

  let slNo = 1;
  let grandTotal = 0;   // computed here, NOT summed in the sheet: the per-session
                        // subtotal rows would be counted twice by a SUM formula.

  for (const sess of sessions) {
    const entries = sess.entries ?? [];
    const osEntries = sess.os_entries ?? [];
    if (entries.length === 0 && osEntries.length === 0) continue;

    const shiftCode = sess.shift === 'DAY' ? 'D' : 'N';
    const dateStr = `${fmtDate(sess.report_date)}(${shiftCode})`;
    const challan = sess.challan_no ?? '';
    const firstRowOfSession = rows.length;
    let prevBr: string | null = null;
    let headerWritten = false;

    for (const e of entries) {
      const isFirst = !headerWritten;
      const isSubRow = Boolean(e.br_no && e.br_no === prevBr);
      // Only carry a REAL br_no forward: a blank spacer row must not make the
      // next genuine BR look like a continuation of the previous one.
      if (e.br_no) prevBr = e.br_no;

      const desc = (e.item_desc || '').toUpperCase().trim();
      const isGold = GOLD_ITEMS.has(desc);
      const isSilver = SILVER_ITEMS.has(desc);
      const entryTotal = e.total_duty ? Math.round(e.total_duty) : 0;
      grandTotal += entryTotal;

      const r: unknown[] = new Array(31).fill(null);
      if (!isSubRow) r[0] = slNo;
      r[1] = isFirst ? challan : '';
      r[2] = isFirst ? dateStr : '';
      r[3] = e.br_no || null;
      r[4] = e.item_desc || null;
      r[5] = e.gold_weight_gms || null;
      // Gold and silver values belong in the metals column, everything else in
      // the baggage column — they are totalled separately in the register.
      r[6] = (isGold || isSilver) ? null : (e.dutiable_value || null);
      r[7] = (isGold || isSilver) ? (e.dutiable_value || null) : null;
      r[8]  = e.baggage_duty || null;
      r[9]  = e.liquor_duty || null;
      r[10] = e.cigarette_duty || null;
      r[11] = e.sw_sc || null;
      r[12] = e.gold_duty_bcd || null;
      r[13] = e.gold_duty_cons || null;
      r[14] = null;                       // silver BCD — no separate field
      r[15] = e.silver_duty_cons || null;
      r[16] = e.sws_on_gold || null;
      r[17] = isGold ? (e.aidc_gold_silver || null) : null;
      r[18] = e.sws_on_silver || null;
      r[19] = isSilver ? (e.aidc_gold_silver || null) : null;
      r[20] = e.aidc_on_liquor || null;
      r[21] = e.redemption_fine || null;
      r[22] = e.reexport_fine || null;
      r[23] = e.personal_penalty || null;
      r[24] = e.other_charges || null;
      r[25] = e.cess_on_cig || null;
      r[26] = e.fuel_duty || null;
      r[27] = entryTotal || null;

      const rowIdx = rows.length;
      rows.push(r);
      for (let c = 0; c < 31; c++) {
        setStyle(rowIdx, c, cellStyle(c, { fill: isFirst ? 'E2EFDA' : undefined }));
      }
      setStyle(rowIdx, 1, cellStyle(1, { bold: isFirst, fill: isFirst ? 'E2EFDA' : undefined }));
      setStyle(rowIdx, 2, cellStyle(2, { bold: isFirst, fill: isFirst ? 'E2EFDA' : undefined }));
      // The distinction the register is read for.
      setStyle(rowIdx, 3, cellStyle(3, {
        color: e.is_offline_br ? '000000' : '0070C0',
        bold: !e.is_offline_br,
        fill: isFirst ? 'E2EFDA' : undefined,
      }));
      setStyle(rowIdx, 27, cellStyle(27, { bold: true, fill: isFirst ? 'E2EFDA' : undefined }));

      if (!isSubRow) slNo++;
      headerWritten = true;
    }

    // Merge runs of the same BR in the BR column, so one receipt covering
    // several items reads as one receipt.
    let i = 0;
    while (i < entries.length) {
      let j = i + 1;
      while (j < entries.length && entries[j].br_no && entries[j].br_no === entries[i].br_no) j++;
      if (j > i + 1 && entries[i].br_no) {
        merges.push({ s: { r: firstRowOfSession + i, c: 3 }, e: { r: firstRowOfSession + j - 1, c: 3 } });
      }
      i = j;
    }

    // OS entries (fuel duty and the like) — amount in the FUEL DUTY column.
    for (const os of osEntries) {
      const isFirst = !headerWritten;
      const osTotal = os.amount ? Math.round(os.amount) : 0;
      grandTotal += osTotal;
      const r: unknown[] = new Array(31).fill(null);
      r[0] = slNo;
      r[1] = isFirst ? challan : '';
      r[2] = isFirst ? dateStr : '';
      r[3] = os.os_no || null;
      r[4] = os.item_desc || null;
      r[26] = os.amount || null;
      r[27] = osTotal || null;
      const rowIdx = rows.length;
      rows.push(r);
      for (let c = 0; c < 31; c++) setStyle(rowIdx, c, cellStyle(c));
      setStyle(rowIdx, 27, cellStyle(27, { bold: true }));
      slNo++;
      headerWritten = true;
    }

    // Offline / online subtotals — how the officer checks the challan amount.
    const offline = Math.round(entries.filter(e => e.is_offline_br)
      .reduce((s, e) => s + (e.total_duty || 0), 0));
    const online = Math.round(entries.filter(e => !e.is_offline_br)
      .reduce((s, e) => s + (e.total_duty || 0), 0));
    const label = challan || 'No Challan';
    for (const [text, amt] of [
      [`↳ Offline [${label}]`, offline],
      ['↳ Online [Portal]', online],
    ] as [string, number][]) {
      const r: unknown[] = new Array(31).fill(null);
      r[4] = text;
      r[27] = amt || null;
      const rowIdx = rows.length;
      rows.push(r);
      for (let c = 0; c < 28; c++) {
        setStyle(rowIdx, c, cellStyle(c, { size: 7, italic: true, bold: c === 4, fill: 'EBF5FB' }));
      }
    }
  }

  // Grand total. Columns 9-27 use SUM, which is safe because the subtotal rows
  // put nothing in them; column 28 uses the figure computed above, because a SUM
  // there WOULD swallow the subtotals and double the month.
  const dataEndRow = rows.length;          // 1-based sheet row of the last data row
  if (dataEndRow > 1) {
    const r: unknown[] = new Array(31).fill(null);
    r[4] = 'GRAND TOTAL';
    r[27] = grandTotal || null;
    const rowIdx = rows.length;
    rows.push(r);
    for (let c = 0; c < 31; c++) {
      setStyle(rowIdx, c, cellStyle(c, { bold: true, fill: 'FFF2CC' }));
    }
  }

  const ws = XLSX.utils.aoa_to_sheet(rows);

  // SUM formulas for the money columns (sheet rows 2 .. dataEndRow).
  if (dataEndRow > 1) {
    const totalRowIdx = rows.length - 1;
    for (let c = 8; c <= 26; c++) {
      const col = XLSX.utils.encode_col(c);
      const addr = XLSX.utils.encode_cell({ r: totalRowIdx, c });
      ws[addr] = { t: 'n', f: `SUM(${col}2:${col}${dataEndRow})`, s: styles.get(addr) };
    }
  }

  // Header styling.
  for (let c = 0; c < MONTHLY_HEADERS.length; c++) {
    const addr = XLSX.utils.encode_cell({ r: 0, c });
    if (ws[addr]) {
      ws[addr].s = {
        font: { sz: 8, bold: true },
        fill: { patternType: 'solid', fgColor: { rgb: 'D9E1F2' } },
        alignment: { horizontal: 'center', vertical: 'center', wrapText: true },
        border: BORDER,
      };
    }
  }
  for (const [addr, st] of styles) if (ws[addr]) ws[addr].s = st;

  ws['!cols'] = MONTHLY_WIDTHS.map(wch => ({ wch }));
  ws['!merges'] = merges;
  ws['!rows'] = rows.map((_, i) => ({ hpt: i === 0 ? 36 : 15 }));
  ws['!freeze'] = { xSplit: 3, ySplit: 1 } as never;

  const monthName = new Date(year, month - 1, 1)
    .toLocaleString('en-GB', { month: 'long' });
  const wb = XLSX.utils.book_new();
  XLSX.utils.book_append_sheet(wb, ws, `${monthName.slice(0, 3)} ${year}`);
  const buf = XLSX.write(wb, { type: 'array', bookType: 'xlsx' });
  triggerDownload(buf, `${monthName} ${year} - Monthly Revenue Report.xlsx`);
}
