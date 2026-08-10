/**
 * Client-side Excel generation for DCR reports using SheetJS.
 * COPS2 generates Excel on the frontend (no server-side openpyxl needed).
 * Data is taken directly from the React state — faster, no extra round-trip.
 */
import type { DrEntry, DrSession } from './revenueCalc';
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

// ── Column definitions (mirrors OS_module_upgrade HEADERS exactly) ────────────

const ENTRY_COLS: (keyof DrEntry)[] = [
  'sl_no', 'br_no', 'is_offline_br', 'os_ref', 'item_desc',
  'dutiable_value', 'gold_weight_gms',
  'baggage_duty', 'liquor_duty', 'cigarette_duty', 'sw_sc',
  'gold_duty_bcd', 'gold_duty_cons', 'silver_duty_cons',
  'sws_on_gold', 'aidc_gold_silver', 'sws_on_silver', 'aidc_on_liquor',
  'redemption_fine', 'reexport_fine', 'personal_penalty', 'other_charges',
  'fuel_duty', 'cess_on_cig', 'total_duty', 'flight_no',
];

const REVENUE_HEADERS = [
  'SR.NO.', 'BR NO.', 'Offline', 'OS No.', 'Item Description',
  'Total Dutiable Value', 'GOLD Weight(gms)',
  'Baggage duty', 'Liquor duty', 'Cigarette duty', 'SW SC',
  'Gold Duty (BCD)', 'Gold Duty (C)', 'Silver Duty (C)',
  'SWS on Gold', 'AIDC Gold/Silver', 'SWS on Silver',
  'AIDC on Liquor', 'Redemption Fine', 'Re-export Fine',
  'Personal Penalty', 'Other Charges', 'Fuel Duty', 'Total Duty',
  'Flight No',
];

const REVENUE_COL_WIDTHS = [
  5, 10, 5, 10, 18, 12, 10, 10, 10, 10, 8,
  10, 10, 10, 8, 10, 8, 8, 10, 8, 10, 8, 6, 10, 10,
];

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

function entryToRevenueRow(e: DrEntry, idx: number): unknown[] {
  return [
    idx + 1,
    e.br_no || '',
    e.is_offline_br ? 'Y' : '',
    e.os_ref || '',
    e.item_desc || '',
    e.dutiable_value || 0,
    e.gold_weight_gms || null,
    e.baggage_duty || 0,
    e.liquor_duty || 0,
    e.cigarette_duty || 0,
    e.sw_sc || 0,
    e.gold_duty_bcd || 0,
    e.gold_duty_cons || 0,
    e.silver_duty_cons || 0,
    e.sws_on_gold || 0,
    e.aidc_gold_silver || 0,
    e.sws_on_silver || 0,
    e.aidc_on_liquor || 0,
    e.redemption_fine || 0,
    e.reexport_fine || 0,
    e.personal_penalty || 0,
    e.other_charges || 0,
    e.fuel_duty || 0,
    e.cess_on_cig || 0,
    e.total_duty || 0,
    e.flight_no || '',
  ];
}

function makeTotalRow(entries: DrEntry[]): unknown[] {
  const row: unknown[] = ['', 'TOTAL', '', '', ''];
  for (let ci = 5; ci <= 23; ci++) {
    const key = ENTRY_COLS[ci] as keyof DrEntry;
    if (key === 'gold_weight_gms') {
      row.push(entries.reduce((s, e) => s + ((e[key] as number) || 0), 0));
    } else {
      row.push(entries.reduce((s, e) => s + ((e[key] as number) || 0), 0));
    }
  }
  row.push(''); // Flight No
  return row;
}

function makeRevenueSheet(XLSX: typeof XLSXTypes, entries: DrEntry[]): XLSXTypes.WorkSheet {
  const dataRows = entries.map((e, i) => entryToRevenueRow(e, i));
  const totalRow = makeTotalRow(entries);
  const ws = XLSX.utils.aoa_to_sheet([REVENUE_HEADERS, ...dataRows, totalRow]);
  ws['!cols'] = REVENUE_COL_WIDTHS.map(wch => ({ wch }));
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

  if (daySession) {
    const label = `DAY (${daySession.batch_name ?? ''})`.slice(0, 31);
    XLSX.utils.book_append_sheet(wb, makeRevenueSheet(XLSX, dayEntries), label);
  }
  if (nightSession) {
    const label = `NIGHT (${nightSession.batch_name ?? ''})`.slice(0, 31);
    XLSX.utils.book_append_sheet(wb, makeRevenueSheet(XLSX, nightEntries), label);
  }

  // Consolidated (all entries)
  const combined = [...dayEntries, ...nightEntries];
  XLSX.utils.book_append_sheet(wb, makeRevenueSheet(XLSX, combined), 'CONSOLIDATED');

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
