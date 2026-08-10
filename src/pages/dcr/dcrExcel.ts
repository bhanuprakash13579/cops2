/**
 * Client-side Excel generation for DCR reports using SheetJS.
 * COPS2 generates Excel on the frontend (no server-side openpyxl needed).
 * Data is taken directly from the React state — faster, no extra round-trip.
 */
import * as XLSX from 'xlsx';
import type { DrEntry, DrSession } from './revenueCalc';
import { fmtDate } from './revenueCalc';
import { showDownloadToast } from '@/components/DownloadToast';

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

function makeRevenueSheet(entries: DrEntry[]): XLSX.WorkSheet {
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
export function downloadAdcExcel(session: DrSession, entries: DrEntry[]): void {
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
export function downloadRevenueExcel(
  reportDate: string,
  daySession: DrSession | null,
  nightSession: DrSession | null,
): void {
  const fname = `${fmtDate(reportDate)} REVENUE REPORT.xlsx`;
  const wb = XLSX.utils.book_new();

  const dayEntries   = daySession?.entries   ?? [];
  const nightEntries = nightSession?.entries ?? [];

  if (daySession) {
    const label = `DAY (${daySession.batch_name ?? ''})`.slice(0, 31);
    XLSX.utils.book_append_sheet(wb, makeRevenueSheet(dayEntries), label);
  }
  if (nightSession) {
    const label = `NIGHT (${nightSession.batch_name ?? ''})`.slice(0, 31);
    XLSX.utils.book_append_sheet(wb, makeRevenueSheet(nightEntries), label);
  }

  // Consolidated (all entries)
  const combined = [...dayEntries, ...nightEntries];
  XLSX.utils.book_append_sheet(wb, makeRevenueSheet(combined), 'CONSOLIDATED');

  const buf = XLSX.write(wb, { type: 'array', bookType: 'xlsx' });
  triggerDownload(buf, fname);
}
