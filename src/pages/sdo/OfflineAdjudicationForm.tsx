/* eslint-disable @typescript-eslint/no-explicit-any */
import { useState, useEffect, useCallback, useMemo, memo, useRef, Fragment } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { ArrowLeft, Plus, Trash2, FileText, User, Plane, AlertCircle, CheckCircle, Info, Upload, Table2, PenLine } from 'lucide-react';
import DatePicker from '@/components/DatePicker';
import api from '@/lib/api';
import * as XLSX from 'xlsx';

// ── Excel import helpers ──────────────────────────────────────────────────────

type ParsedItem = { items_desc: string; items_qty: number; items_uqc: string; items_value: number; items_duty_type: string };

/** Parse "2.5 KGS" or "1 NOS" → { qty, uqc } */
function parseQtyUqc(raw: string): { qty: number; uqc: string } {
  const m = raw.trim().match(/^([\d.]+)\s*([A-Za-z]+)?$/);
  if (m) return { qty: parseFloat(m[1]) || 1, uqc: (m[2] || 'NOS').toUpperCase().slice(0, 3) };
  return { qty: 1, uqc: 'NOS' };
}

/**
 * Parse item description + quantity columns into a structured items list.
 *
 * Handles three formats:
 *  1. Numbered list  — "1) GOLD BISCUIT 2) IPHONE 17"
 *  2. Comma-separated with leading qty  — "2 GAMING CARD,5000 RESISTOR 0603 300R 1% 150MW"
 *  3. Single item (fall-through)
 *
 * Returns `needsManualEntry: true` when some tokens had a leading qty and
 * some did not (ambiguous split) — the caller should ask the user to confirm.
 */
function parseExcelItems(descCol: string, qtyCol: string): { items: ParsedItem[]; needsManualEntry: boolean } {
  if (!descCol) return { items: [], needsManualEntry: false };

  // 1. Numbered list: "1) DESC 2) DESC"
  const numberedParts = descCol.split(/\d+\)\s+/).filter(s => s.trim().length > 0);
  if (numberedParts.length > 1) {
    const qtyParts = (qtyCol || '').split(/\d+\)\s+/).filter(s => s.trim().length > 0);
    return {
      items: numberedParts.map((d, i) => {
        const { qty, uqc } = parseQtyUqc(qtyParts[i] || '1 NOS');
        return { items_desc: d.trim().toUpperCase(), items_qty: qty, items_uqc: uqc, items_value: 0, items_duty_type: 'Miscellaneous-22' };
      }),
      needsManualEntry: false,
    };
  }

  // 2. Comma-separated "QTY DESC, QTY DESC" (handles complex specs like "5000 RESISTOR 0603 300R 1% 150MW")
  const parts = descCol.split(',').map(s => s.trim()).filter(Boolean);
  if (parts.length > 1) {
    const QTY_PREFIX = /^(\d+(?:\.\d+)?)\s+(.+)$/;
    const parsed = parts.map(p => {
      const m = p.match(QTY_PREFIX);
      return m
        ? { qty: parseFloat(m[1]) || 1, desc: m[2].trim(), matched: true as const }
        : { qty: 1, desc: p.trim(), matched: false as const };
    });
    const matchCount = parsed.filter(p => p.matched).length;
    if (matchCount === parsed.length) {
      // All tokens have a leading qty — clean, unambiguous parse
      return {
        items: parsed.map(p => ({ items_desc: p.desc.toUpperCase(), items_qty: p.qty, items_uqc: 'NOS', items_value: 0, items_duty_type: 'Miscellaneous-22' })),
        needsManualEntry: false,
      };
    }
    if (matchCount > 0) {
      // Mixed — pre-populate from what parsed but ask user to review
      return {
        items: parsed.map(p => ({ items_desc: p.desc.toUpperCase(), items_qty: p.qty, items_uqc: 'NOS', items_value: 0, items_duty_type: 'Miscellaneous-22' })),
        needsManualEntry: true,
      };
    }
    // No leading qty anywhere — treat entire string as one item
    return {
      items: [{ items_desc: descCol.toUpperCase(), items_qty: 1, items_uqc: 'NOS', items_value: 0, items_duty_type: 'Miscellaneous-22' }],
      needsManualEntry: false,
    };
  }

  // 3. Single item — apply qty column if present
  const { qty, uqc } = qtyCol ? parseQtyUqc(qtyCol) : { qty: 1, uqc: 'NOS' };
  return {
    items: [{ items_desc: descCol.toUpperCase(), items_qty: qty, items_uqc: uqc, items_value: 0, items_duty_type: 'Miscellaneous-22' }],
    needsManualEntry: false,
  };
}

/**
 * Classify "Column 1" text into an RF/REF/Confiscated outcome type.
 * Empty → 'none'   (AIU cases / no classification yet — allowed through import)
 * Unknown text → 'ambiguous'  (user must select before importing)
 */
function classifyColumn1(col1: string): 'rf' | 'ref' | 'confiscated' | 'none' | 'ambiguous' {
  const v = (col1 || '').toLowerCase().trim();
  if (!v) return 'none';
  if (v.includes('absolute confiscation')) return 'confiscated';
  if (v.includes('confiscation'))          return 'rf';
  if (v.match(/re.?export/))               return 'ref';
  return 'ambiguous';
}

/** Convert Excel serial date number to YYYY-MM-DD, or pass through ISO string */
function excelDateToIso(raw: any): string {
  if (!raw) return '';
  if (typeof raw === 'number') {
    // Excel serial date: days since 1899-12-30
    const d = new Date(Math.round((raw - 25569) * 86400 * 1000));
    return d.toISOString().split('T')[0];
  }
  const s = String(raw).trim();
  // DD/MM/YYYY
  const dmy = s.match(/^(\d{1,2})[\/\-](\d{1,2})[\/\-](\d{2,4})$/);
  if (dmy) {
    const y = dmy[3].length === 2 ? `20${dmy[3]}` : dmy[3];
    return `${y}-${dmy[2].padStart(2, '0')}-${dmy[1].padStart(2, '0')}`;
  }
  // Already ISO-ish
  if (/^\d{4}-\d{2}-\d{2}/.test(s)) return s.slice(0, 10);
  return '';
}

interface ParsedImportRow {
  sno:              number;
  os_no:            string;
  os_date:          string;
  os_year:          number;
  booked_by:        string;
  flight_no:        string;
  pax_name:         string;
  pax_nationality:  string;
  passport_no:      string;
  pax_address1:     string;
  file_spot:        string;
  case_type:        string;
  items:            Array<{ items_desc: string; items_qty: number; items_uqc: string; items_value: number; items_duty_type: string }>;
  total_items_value: number;
  total_duty_amount: number;
  total_payable:    number;
  rf_amount:        number;
  ref_amount:       number;
  pp_amount:        number;
  confiscated_value: number;
  adj_offr_name:    string;
  adj_offr_designation: string;
  adjudication_date: string;
  post_adj_br_entries: string;
  // RF/REF classification
  // 'none' = AIU/no classification (allowed through), 'ambiguous' = user must select
  rfRefType:        'rf' | 'ref' | 'confiscated' | 'none' | 'ambiguous';
  rfRefValue:       number;
  needsManualItems: boolean;  // true when item parsing was ambiguous — user must confirm
  // Parse warnings
  warnings:         string[];
}

function parseExcelRows(sheetData: any[][]): ParsedImportRow[] {
  if (sheetData.length < 2) return [];

  // Normalize header row to lowercase, trim
  const rawHeaders = (sheetData[0] || []).map((h: any) => String(h || '').toLowerCase().trim());

  // Column aliases (handle both old and new header variants)
  const headerAliases: Record<string, string[]> = {
    'os no':           ['os no', 'os_no', 'os number', 's.no'],
    'os date':         ['os date', 'os_date', 'date'],
    'batch / aiu':     ['batch / aiu', 'batch/aiu', 'booked by', 'batch'],
    'flt no':          ['flt no', 'flt_no', 'flight no', 'flight_no'],
    'pax name':        ['pax name', 'pax_name', 'passenger name'],
    'nationality':     ['nationality', 'pax_nationality', 'pax nationality'],
    'passport no.':    ['passport no.', 'passport no', 'passport_no'],
    'address':         ['address', 'pax_address1', 'pax address'],
    'item description':['item description', 'item_description', 'items_desc'],
    'quantity':        ['quantity', 'qty'],
    'value in rs.':    ['value in rs.', 'value in rs', 'value_in_rs', 'total value'],
    'rf/r.e.f':        ['rf/r.e.f', 'rf/ref', 'rf_amount', 'ref_amount'],
    'penalty':         ['penalty', 'pp_amount'],
    'duty in rs':      ['duty in rs', 'duty_in_rs', 'duty rs', 'total_duty_amount'],
    'total':           ['total', 'total_payable'],
    'b.r no':          ['b.r no', 'br no', 'br_no'],
    'b.r date':        ['b.r date', 'br date', 'br_date'],
    'file / spot -adjudication': ['file / spot -adjudication', 'file/spot', 'file_spot'],
    'adjudicated by ac/dc':      ['adjudicated by ac/dc', 'adj_offr_name', 'adjudicated by'],
    'adjudicated by jc/adc':     ['adjudicated by jc/adc', 'adj_offr_designation'],
    'export/import':             ['export/import', 'case_type'],
    'column 1':                  ['column 1', 'column1', 'col1', 'confiscation type'],
  };

  // Build a resolved col-index map
  const colMap: Record<string, number> = {};
  for (const [canonical, aliases] of Object.entries(headerAliases)) {
    for (const alias of aliases) {
      const idx = rawHeaders.indexOf(alias);
      if (idx >= 0) { colMap[canonical] = idx; break; }
    }
  }

  const getCol = (row: any[], canonical: string) => {
    const i = colMap[canonical];
    return i !== undefined ? String(row[i] ?? '').trim() : '';
  };
  const getColNum = (row: any[], canonical: string) => {
    const i = colMap[canonical];
    if (i === undefined) return 0;
    const v = row[i];
    return typeof v === 'number' ? v : parseFloat(String(v || '0').replace(/,/g, '')) || 0;
  };

  const results: ParsedImportRow[] = [];

  for (let ri = 1; ri < sheetData.length; ri++) {
    const row = sheetData[ri];
    if (!row || row.every((c: any) => !c)) continue; // skip blank rows

    const warnings: string[] = [];

    const os_no_raw = getCol(row, 'os no').replace(/\D/g, ''); // strip non-digits
    if (!os_no_raw) { warnings.push('No OS No. — row skipped'); continue; }

    const os_date_raw = getCol(row, 'os date');
    const os_date = excelDateToIso(colMap['os date'] !== undefined ? row[colMap['os date']] : os_date_raw);
    if (!os_date) warnings.push('Could not parse OS Date');
    const os_year = os_date ? parseInt(os_date.split('-')[0]) : new Date().getFullYear();

    const descRaw = getCol(row, 'item description');
    const qtyRaw  = getCol(row, 'quantity');
    const { items, needsManualEntry } = descRaw
      ? parseExcelItems(descRaw, qtyRaw)
      : { items: [], needsManualEntry: false };
    if (items.length === 0) warnings.push('No item description found');
    if (needsManualEntry) warnings.push('Item descriptions need review — confirm items before importing');

    // Distribute total value across items proportionally if we have a total
    const totalValue = getColNum(row, 'value in rs.');
    if (items.length === 1) {
      items[0].items_value = totalValue;
    } else if (items.length > 1) {
      // Equal split — value per item
      const perItem = Math.round((totalValue / items.length) * 100) / 100;
      items.forEach(itm => { itm.items_value = perItem; });
    }

    const rfRefRaw = getColNum(row, 'rf/r.e.f');
    const col1     = getCol(row, 'column 1');
    const rfRefType = classifyColumn1(col1);
    if (rfRefType === 'ambiguous') warnings.push('RF/REF type ambiguous — please select below');
    // 'none' gets no warning — expected for AIU cases

    let rf_amount = 0, ref_amount = 0, confiscated_value = 0;
    if (rfRefType === 'rf')          rf_amount = rfRefRaw;
    else if (rfRefType === 'ref')    ref_amount = rfRefRaw;
    else if (rfRefType === 'confiscated') confiscated_value = rfRefRaw;

    // BR entries
    const brNo   = getCol(row, 'b.r no');
    const brDate = excelDateToIso(colMap['b.r date'] !== undefined ? row[colMap['b.r date']] : '');
    const post_adj_br_entries = brNo
      ? JSON.stringify([{ no: brNo, date: brDate || null }])
      : '';

    const fileSpotRaw = getCol(row, 'file / spot -adjudication').toLowerCase();
    const file_spot = fileSpotRaw.includes('file') ? 'File' : 'Spot';

    const caseTypeRaw = getCol(row, 'export/import').toLowerCase();
    const case_type = caseTypeRaw.includes('export') ? 'Export Case' : 'Arrival Case';

    const adjDesigRaw = getCol(row, 'adjudicated by jc/adc').toLowerCase();
    const adj_offr_designation = adjDesigRaw.includes('jc') ? 'JC'
      : adjDesigRaw.includes('adc') ? 'ADC'
      : adjDesigRaw.includes('ac') ? 'AC'
      : adjDesigRaw.includes('dc') ? 'DC'
      : getCol(row, 'adjudicated by jc/adc');

    results.push({
      sno:             ri,
      os_no:           os_no_raw,
      os_date,
      os_year,
      booked_by:       getCol(row, 'batch / aiu') || 'BATCH A',
      flight_no:       getCol(row, 'flt no').toUpperCase(),
      pax_name:        getCol(row, 'pax name').toUpperCase(),
      pax_nationality: getCol(row, 'nationality').toUpperCase(),
      passport_no:     getCol(row, 'passport no.').toUpperCase(),
      pax_address1:    getCol(row, 'address').toUpperCase(),
      file_spot,
      case_type,
      items,
      total_items_value: totalValue,
      total_duty_amount: getColNum(row, 'duty in rs'),
      total_payable:     getColNum(row, 'total'),
      rf_amount,
      ref_amount,
      pp_amount:         getColNum(row, 'penalty'),
      confiscated_value,
      adj_offr_name:     getCol(row, 'adjudicated by ac/dc').toUpperCase(),
      adj_offr_designation,
      adjudication_date: os_date,
      post_adj_br_entries,
      rfRefType,
      rfRefValue:        rfRefRaw,
      needsManualItems:  needsManualEntry,
      warnings,
    });
  }

  return results;
}

// ── Static seed list for item-description autocomplete ───────────────────────
const STATIC_ITEM_SUGGESTIONS = [
  'CIGARETTES','E-CIGARETTES','CIGARS','BIDI','GUTKHA','PAN MASALA','TOBACCO',
  'WHISKY','BRANDY','WINE','BEER','VODKA','RUM','GIN','SCOTCH','CHAMPAGNE','TEQUILA','LIQUOR',
  'GOLD (JEWELLERY)','GOLD (PRIMARY)','GOLD BAR','GOLD BISCUIT','GOLD CHAIN','GOLD RING',
  'SILVER (JEWELLERY)','SILVER BAR',
  'CURRENCY (FOREIGN)','CURRENCY (INDIAN)',
  'MOBILE PHONE','CELL PHONE','IPHONE','LAPTOP','TABLET','SMART WATCH',
  'CAMERA','VIDEO CAMERA','DRONE','ELECTRONIC GOODS',
  'NARCOTICS (CANNABIS/GANJA)','NARCOTICS (HEROIN)','NARCOTICS (COCAINE)',
  'ARMS','AMMUNITION','EXPLOSIVES',
  'ANTIQUES','TOYS','TEXTILES','FABRICS','COSMETICS','PERFUMES','SUNGLASSES','WATCHES',
  'RED SANDERS','POPPY SEEDS','POPPY HUSK',
  'REFURBISHED LAPTOP','REFURBISHED MOBILE PHONE',
  'MARLBORO CIGARETTES','DUNHILL CIGARETTES','GUDANG GARAM CIGARETTES',
  'CHIVAS REGAL WHISKY','JOHNNIE WALKER WHISKY','BARDINET BRANDY','JACK DANIELS WHISKY',
  'MEDICINES','FOOD ITEMS','DRY FRUITS','SPICES','LEATHER GOODS','GARMENTS',
];

const DUTY_TYPES = [
  "Antiques-01", "Audio CDs-02", "Cigarettes-03", "Currency (Foreign)-04", "Currency (FICN)-05",
  "Gold (Jewellery)-06", "Gold (Primary)-07", "Liquor-08", "Narcotics (Cannabis/Ganja)-09",
  "Narcotics (Heroin/Brown Sugar)-10", "Narcotics (Cocaine)-11", "Live Species / Wildlife-12",
  "Arms & Ammunition-13", "Silver-14", "Semi Precious / Precious Stones-15", "Video CDs-16",
  "Cameras / Video Cameras-17", "Cell Phones-18", "Cordless Phones-19", "Calculator & Digital Diary-20",
  "Electronic Goods-21", "Miscellaneous-22", "VCD / DVD Players-23", "Walkmans-24", "Watch / Watch Movements-25",
  "Textiles / Fabrics-26", "FEMA (Foreign Exchange)-27", "Commercial Fraud (Imports)-28",
  "Commercial Fraud (Exports)-29", "Tobacco / Gutkha-30", "Morphine-31", "Opium-32", "Psychotropic Substances-33",
  "Ephedrine / Precursors-34", "Fake Indian Goods / IPR-35", "Red Sanders / Timber-36",
  "Ivory / Elephant Products-37", "Pangolin / Animal Parts-38", "Coral / Marine Products-39",
  "Prohibited Imports-40", "Prohibited Exports-41", "Duty Evasion (Imports)-42", "Duty Evasion (Exports)-43",
  "Misdeclaration (Imports)-44", "Misdeclaration (Exports)-45", "Under-valuation (Imports)-46",
  "Under-valuation (Exports)-47", "Overvaluation (Exports)-48", "Drawback Fraud-49",
  "EPCG / Advance Licence Fraud-50", "FTA / Preferential Duty Fraud-51", "Narcotics (Methamphetamine/Synthetic)-52",
  "Narcotics (Ketamine/NPS)-53", "Narcotics (Mandrax/Methaqualone)-54", "Narcotics (Other NDPS)-55",
  "Narcotic (Imports)-56", "Narcotic (Exports)-57", "Explosives-58", "Dual Use / SCOMET Goods-59",
  "Human Trafficking-60", "Hazardous Waste-61", "E-Waste-62", "Areca Nut-63", "Betel Leaves-64",
  "Wildlife (CITES)-65", "ODS (Exports)-66", "ODS (Imports)-67", "Counterfeit Currency-68",
  "Counterfeit Goods-69", "Other_Baggage-99"
];

const DUTY_TYPE_OPTIONS = DUTY_TYPES.map(type => (
  <option key={type} value={type}>{type}</option>
));

const sanitizeInteger = (raw: string) => raw.replace(/[^\d]/g, '');
const sanitizeDecimal = (raw: string) => {
  const cleaned = raw.replace(/[^\d.]/g, '');
  const firstDot = cleaned.indexOf('.');
  const result = firstDot === -1
    ? cleaned
    : cleaned.slice(0, firstDot + 1) + cleaned.slice(firstDot + 1).replace(/\./g, '');
  return result.replace(/^0+([1-9])/, '$1');
};

// ── Simplified Item Row (no FA, no release category) ─────────────────────────
interface SimpleItemRowProps {
  itm: any;
  idx: number;
  rowErrors: Record<string, string> | undefined;
  updateItem: (idx: number, field: string, value: any) => void;
  onRemove: (idx: number) => void;
  onDescBlur: (idx: number, desc: string) => void;
  descDatalistId: string;
}

const SimpleItemRow = memo(function SimpleItemRow({
  itm, idx, rowErrors, updateItem, onRemove, onDescBlur, descDatalistId
}: SimpleItemRowProps) {
  return (
    <tr id={`item-row-${idx}`} className="hover:bg-slate-50 group">
      <td className="px-3 py-2 text-center font-medium text-slate-500 text-sm">{idx + 1}</td>
      <td className="px-2 py-1.5">
        <input
          type="text"
          list={descDatalistId}
          autoComplete="off"
          className={`w-full px-2 py-1.5 border ${rowErrors?.items_desc ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300'} rounded text-xs uppercase focus:outline-none focus:ring-2 focus:ring-blue-400`}
          value={itm.items_desc}
          onChange={e => updateItem(idx, 'items_desc', e.target.value.toUpperCase())}
          onBlur={e => onDescBlur(idx, e.target.value)}
        />
        {rowErrors?.items_desc && <p className="text-[10px] text-red-500 mt-0.5">{rowErrors.items_desc}</p>}
      </td>
      <td className="px-2 py-1.5">
        <div className="flex items-center gap-1">
          <input
            type="text"
            inputMode="decimal"
            className={`w-14 px-1.5 py-1.5 border ${rowErrors?.items_qty ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300'} rounded text-xs text-center focus:outline-none focus:ring-2 focus:ring-blue-400`}
            value={itm.items_qty || ''}
            placeholder="0"
            onChange={e => updateItem(idx, 'items_qty', sanitizeDecimal(e.target.value))}
          />
          <select
            className="w-16 px-1 py-1.5 border border-slate-300 rounded text-[10px] focus:outline-none focus:ring-1 focus:ring-blue-400"
            value={itm.items_uqc}
            onChange={e => updateItem(idx, 'items_uqc', e.target.value)}
          >
            <option value="NOS">Nos.</option>
            <option value="STK">Sticks</option>
            <option value="KGS">Kgs.</option>
            <option value="GMS">Gms.</option>
            <option value="LTR">Ltrs.</option>
            <option value="MTR">Mtrs.</option>
            <option value="PRS">Pairs</option>
          </select>
        </div>
        {rowErrors?.items_qty && <p className="text-[10px] text-red-500 mt-0.5">{rowErrors.items_qty}</p>}
      </td>
      <td className="px-2 py-1.5">
        <input
          type="text"
          inputMode="decimal"
          className={`w-full px-2 py-1.5 border ${rowErrors?.items_value ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300'} rounded text-xs text-right focus:outline-none focus:ring-2 focus:ring-blue-400`}
          value={itm.items_value || ''}
          placeholder="0"
          onChange={e => updateItem(idx, 'items_value', sanitizeDecimal(e.target.value))}
        />
        {rowErrors?.items_value && <p className="text-[10px] text-red-500 mt-0.5">{rowErrors.items_value}</p>}
      </td>
      <td className="px-2 py-1.5">
        <select
          className={`w-full px-2 py-1.5 border ${rowErrors?.items_duty_type ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300'} rounded text-xs focus:outline-none focus:ring-2 focus:ring-blue-400`}
          value={itm.items_duty_type}
          onChange={e => updateItem(idx, 'items_duty_type', e.target.value)}
        >
          {DUTY_TYPE_OPTIONS}
        </select>
        {rowErrors?.items_duty_type && <p className="text-[10px] text-red-500 mt-0.5">{rowErrors.items_duty_type}</p>}
      </td>
      <td className="px-2 py-1.5 text-center">
        <button
          onClick={(e) => { e.preventDefault(); onRemove(idx); }}
          className="text-slate-400 hover:text-red-500 transition-colors"
          title="Remove row"
        >
          <Trash2 size={15} />
        </button>
      </td>
    </tr>
  );
});

// ── Inline item editor for Excel import rows ─────────────────────────────────
interface ItemEditPanelProps {
  initialItems: ParsedItem[];
  totalValue: number;
  penalty: number;
  onConfirm: (items: ParsedItem[]) => void;
  onCancel: () => void;
}
const ItemEditPanel = memo(function ItemEditPanel({ initialItems, totalValue, penalty, onConfirm, onCancel }: ItemEditPanelProps) {
  const seed = initialItems.length > 0
    ? initialItems
    : [{ items_desc: '', items_qty: 1, items_uqc: 'NOS', items_value: 0, items_duty_type: 'Miscellaneous-22' }];
  const [editItems, setEditItems] = useState<ParsedItem[]>(seed);

  const updateItem = useCallback((idx: number, field: string, value: any) => {
    setEditItems(prev => prev.map((item, i) => i === idx ? { ...item, [field]: value } : item));
  }, []);
  const removeItem = useCallback((idx: number) => {
    setEditItems(prev => prev.filter((_, i) => i !== idx));
  }, []);
  const noopBlur = useCallback(() => {}, []);

  // Expand any item whose description has commas/semicolons into individual rows.
  // Each split part inherits an equal share of the parent item's value so the
  // aggregated total remains balanced without the user having to redistribute.
  const autoSplit = useCallback(() => {
    setEditItems(prev => {
      const expanded: ParsedItem[] = [];
      for (const item of prev) {
        const parts = item.items_desc.split(/[,;]/).map(s => s.trim()).filter(Boolean);
        if (parts.length > 1) {
          const perValue = Math.round(((Number(item.items_value) || 0) / parts.length) * 100) / 100;
          parts.forEach(desc => expanded.push({ ...item, items_desc: desc.toUpperCase(), items_qty: 1, items_value: perValue }));
        } else {
          expanded.push(item);
        }
      }
      return expanded;
    });
  }, []);

  // Divide totalValue evenly across all current items
  const distributeEqually = useCallback(() => {
    setEditItems(prev => {
      if (!prev.length) return prev;
      const per = Math.round((totalValue / prev.length) * 100) / 100;
      return prev.map(it => ({ ...it, items_value: per }));
    });
  }, [totalValue]);

  const canConfirm = editItems.length > 0 && editItems.every(it => String(it.items_desc || '').trim().length > 0);
  const distributed = editItems.reduce((s, it) => s + (Number(it.items_value) || 0), 0);
  const valueBalanced = Math.abs(distributed - totalValue) < 1;
  const hasSplittable = editItems.some(it => /[,;]/.test(it.items_desc));

  return (
    <div className="bg-blue-50 border border-blue-200 rounded-lg p-3 mt-1">
      <div className="flex items-start justify-between mb-2 flex-wrap gap-2">
        <div>
          <p className="text-xs font-bold text-blue-800">
            Edit items — Value: ₹{totalValue.toLocaleString('en-IN')}
            {penalty > 0 && <span className="ml-2 text-slate-600 font-normal">· Penalty: ₹{penalty.toLocaleString('en-IN')}</span>}
            {!valueBalanced && (
              <span className="ml-2 text-amber-600 font-normal text-[10px]">
                (distributed: ₹{distributed.toLocaleString('en-IN')})
              </span>
            )}
          </p>
          <p className="text-[10px] text-slate-500 mt-0.5">
            Split merged descriptions, set individual qty / value / duty type, then confirm.
          </p>
        </div>
        <div className="flex gap-1.5 flex-wrap">
          {hasSplittable && (
            <button type="button" onClick={autoSplit}
              className="text-[10px] px-2 py-1 border border-violet-300 text-violet-700 rounded hover:bg-violet-50 font-medium whitespace-nowrap">
              ✂ Auto-split on commas
            </button>
          )}
          <button type="button" onClick={distributeEqually}
            className="text-[10px] px-2 py-1 border border-blue-300 text-blue-700 rounded hover:bg-blue-100 font-medium whitespace-nowrap">
            ÷ Distribute value equally
          </button>
        </div>
      </div>
      <div className="overflow-auto">
        <table className="w-full text-xs">
          <thead className="text-[10px] text-slate-500 uppercase bg-slate-100 border-b border-slate-200 tracking-wider">
            <tr>
              <th className="px-2 py-1.5 w-8 text-center">S.No</th>
              <th className="px-2 py-1.5">Description</th>
              <th className="px-2 py-1.5 w-32 text-center">Qty &amp; Unit</th>
              <th className="px-2 py-1.5 w-24 text-right">Value (₹)</th>
              <th className="px-2 py-1.5 w-36">Duty Type</th>
              <th className="px-2 py-1.5 w-8"></th>
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-100 bg-white">
            {editItems.map((itm, idx) => (
              <SimpleItemRow
                key={idx}
                itm={itm}
                idx={idx}
                rowErrors={undefined}
                updateItem={updateItem}
                onRemove={removeItem}
                onDescBlur={noopBlur}
                descDatalistId="offline-item-desc-datalist"
              />
            ))}
          </tbody>
        </table>
      </div>
      <div className="flex items-center gap-2 mt-2 flex-wrap">
        <button type="button"
          onClick={() => setEditItems(prev => [...prev, { items_desc: '', items_qty: 1, items_uqc: 'NOS', items_value: 0, items_duty_type: 'Miscellaneous-22' }])}
          className="text-xs px-2 py-1 border border-blue-300 text-blue-700 rounded hover:bg-blue-50 font-medium">
          + Add Item
        </button>
        <button type="button" onClick={() => onConfirm(editItems)} disabled={!canConfirm}
          className="text-xs px-3 py-1 bg-green-700 text-white rounded hover:bg-green-600 disabled:opacity-50 font-semibold">
          ✓ Confirm Items
        </button>
        <button type="button" onClick={onCancel}
          className="text-xs px-2 py-1 border border-slate-200 text-slate-600 rounded hover:bg-slate-50">
          Cancel
        </button>
        {!valueBalanced && (
          <span className="text-[10px] text-amber-600 ml-auto">
            ⚠ ₹{Math.abs(distributed - totalValue).toFixed(2)} unallocated
          </span>
        )}
      </div>
    </div>
  );
});

// Module-level cache for item descriptions
let _offlineDescCache: string[] | null = null;

// ─────────────────────────────────────────────────────────────────────────────
export default function OfflineAdjudicationForm() {
  const navigate = useNavigate();
  const { osNo, osYear } = useParams<{ osNo?: string; osYear?: string }>();
  const isEditing = !!(osNo && osYear);

  // ── Item description suggestions ─────────────────────────────────────────
  const [descSuggestions, setDescSuggestions] = useState<string[]>(_offlineDescCache ?? STATIC_ITEM_SUGGESTIONS);
  useEffect(() => {
    if (_offlineDescCache !== null) return;
    api.get('/os/item-descriptions')
      .then(res => {
        if (!Array.isArray(res.data)) return;
        const dbItems: string[] = res.data.map((s: string) => (s || '').toUpperCase()).filter(Boolean);
        const merged = [...dbItems];
        const existing = new Set(dbItems);
        for (const s of STATIC_ITEM_SUGGESTIONS) { if (!existing.has(s)) { merged.push(s); existing.add(s); } }
        _offlineDescCache = merged;
        setDescSuggestions(merged);
      })
      .catch(() => { /* keep static list on error */ });
  }, []);

  const descDatalist = useMemo(() => (
    <datalist id="offline-item-desc-datalist">
      {descSuggestions.map(s => <option key={s} value={s} />)}
    </datalist>
  ), [descSuggestions]);

  // ── Form state ────────────────────────────────────────────────────────────
  const [formData, setFormData] = useState({
    os_no: '',
    os_date: new Date().toISOString().split('T')[0],
    booked_by: 'Batch A',
    flight_no: '',
    pax_name: '',
    pax_nationality: '',
    passport_no: '',
    pax_address1: '',
    file_spot: '',
  });

  const [optionalData, setOptionalData] = useState({
    pax_date_of_birth: '',
    passport_date: '',
    pp_issue_place: '',
    father_name: '',
    residence_at: '',
    old_passport_no: '',
    case_type: 'Non-Bonafide',
    shift: 'Day',
    supdts_remarks: '',
  });

  const [items, setItems] = useState<any[]>([{
    items_desc: '',
    items_qty: 1,
    items_uqc: 'NOS',
    items_value: 0,
    items_duty_type: 'Miscellaneous-22',
  }]);

  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const [itemErrors, setItemErrors] = useState<Record<number, Record<string, string>>>({});
  const [errorMsg, setErrorMsg] = useState('');
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [successInfo, setSuccessInfo] = useState<{ os_no: string; os_year: number } | null>(null);
  const [autoFillBanner, setAutoFillBanner] = useState(false);
  const [additionalOpen, setAdditionalOpen] = useState(false);
  const [confirmSave, setConfirmSave] = useState(false);
  const [editLoading, setEditLoading] = useState(isEditing);
  const [inputMode, setInputMode] = useState<'manual' | 'excel'>('manual');
  const [parsedRows, setParsedRows] = useState<ParsedImportRow[]>([]);
  const [rowRfRefOverrides, setRowRfRefOverrides] = useState<Record<number, 'rf' | 'ref' | 'confiscated'>>({});
  const [rowItemOverrides, setRowItemOverrides] = useState<Record<number, ParsedItem[]>>({});
  const [expandedItemEdit, setExpandedItemEdit] = useState<Set<number>>(new Set());
  const [importLoading, setImportLoading] = useState(false);
  const [importStatus, setImportStatus] = useState<{ imported: number; skipped: number; failed: any[]; total: number } | null>(null);

  // ── Load existing case when editing ───────────────────────────────────────
  useEffect(() => {
    if (!isEditing) return;
    setEditLoading(true);
    api.get(`/os/${osNo}/${osYear}`)
      .then(res => {
        const d = res.data;
        setFormData({
          os_no: d.os_no || '',
          os_date: d.os_date || new Date().toISOString().split('T')[0],
          booked_by: d.booked_by || 'Batch A',
          flight_no: d.flight_no || '',
          pax_name: d.pax_name || '',
          pax_nationality: d.pax_nationality || '',
          passport_no: d.passport_no || '',
          pax_address1: d.pax_address1 || '',
          file_spot: d.file_spot || '',
        });
        setOptionalData({
          pax_date_of_birth: d.pax_date_of_birth || '',
          passport_date: d.passport_date || '',
          pp_issue_place: d.pp_issue_place || '',
          father_name: d.father_name || '',
          residence_at: d.residence_at || '',
          old_passport_no: d.old_passport_no || '',
          case_type: d.case_type || 'Non-Bonafide',
          shift: d.shift || 'Day',
          supdts_remarks: d.supdts_remarks || '',
        });
        if (d.items && d.items.length > 0) {
          setItems(d.items.map((itm: any) => ({
            items_desc: itm.items_desc || '',
            items_qty: itm.items_qty ?? 1,
            items_uqc: itm.items_uqc || 'NOS',
            items_value: itm.items_value ?? 0,
            items_duty_type: itm.items_duty_type || 'Miscellaneous-22',
          })));
        }
      })
      .catch(err => {
        let detail = err.response?.data?.detail || 'Failed to load case for editing.';
        if (Array.isArray(detail)) detail = detail.map((e: any) => `${e.loc?.join('.')} - ${e.msg}`).join(', ');
        else if (typeof detail === 'object') detail = JSON.stringify(detail);
        setErrorMsg(detail);
      })
      .finally(() => setEditLoading(false));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isEditing, osNo, osYear]);

  const osNoCheckTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const ppLookupAbort = useRef<AbortController | null>(null);

  // ── Field error helpers ───────────────────────────────────────────────────
  const setFieldError = useCallback((field: string, message: string) => {
    setFieldErrors(prev => ({ ...prev, [field]: message }));
  }, []);

  const clearFieldError = useCallback((field: string) => {
    setFieldErrors(prev => {
      if (!prev[field]) return prev;
      const { [field]: _removed, ...rest } = prev;
      return rest;
    });
  }, []);

  // ── Item handlers ─────────────────────────────────────────────────────────
  const updateItem = useCallback((idx: number, field: string, value: any) => {
    setItems(prev => prev.map((item, i) => i === idx ? { ...item, [field]: value } : item));
  }, []);

  const onRemove = useCallback((idx: number) => {
    setItems(prev => prev.filter((_, i) => i !== idx));
  }, []);

  // Smart classification on description blur
  const classifyAbortRefs = useRef<Record<number, AbortController>>({});
  useEffect(() => () => { Object.values(classifyAbortRefs.current).forEach(c => c.abort()); }, []);

  const onDescBlur = useCallback(async (idx: number, desc: string) => {
    if (!desc || desc.trim().length < 3) return;
    classifyAbortRefs.current[idx]?.abort();
    const ctrl = new AbortController();
    classifyAbortRefs.current[idx] = ctrl;
    try {
      const res = await api.get('/os/classify-item', { params: { description: desc }, signal: ctrl.signal });
      if (res.data?.duty_type && res.data.duty_type !== 'Miscellaneous-22') {
        setItems(prev => {
          const row = prev[idx];
          if (!row) return prev;
          const updated = { ...row, items_duty_type: res.data.duty_type };
          if (res.data.uqc && res.data.uqc !== 'NOS') updated.items_uqc = res.data.uqc;
          return prev.map((item, i) => i === idx ? updated : item);
        });
      }
    } catch { /* silent */ }
  }, []);

  // ── Passport lookup on blur ───────────────────────────────────────────────
  const handlePassportBlur = useCallback(async (pp: string) => {
    if (!pp || pp.trim().length < 4) return;
    ppLookupAbort.current?.abort();
    const ctrl = new AbortController();
    ppLookupAbort.current = ctrl;
    try {
      const res = await api.post('/passports/lookup', {
        passport_no: pp.trim().toUpperCase(),
      }, { signal: ctrl.signal });
      const data = res.data;
      if (!data || !data.pax_name) return;

      // Only fill fields that are currently empty
      setFormData(prev => ({
        ...prev,
        pax_name: prev.pax_name || (data.pax_name || ''),
        pax_nationality: prev.pax_nationality || (data.pax_nationality || ''),
        pax_address1: prev.pax_address1 || (data.pax_address1 || ''),
      }));
      setOptionalData(prev => ({
        ...prev,
        pax_date_of_birth: prev.pax_date_of_birth || (data.pax_date_of_birth || ''),
        passport_date: prev.passport_date || (data.passport_date || ''),
        pp_issue_place: prev.pp_issue_place || (data.pp_issue_place || ''),
        father_name: prev.father_name || (data.father_name || ''),
        residence_at: prev.residence_at || (data.residence_at || ''),
        old_passport_no: prev.old_passport_no || (data.old_passport_no || ''),
      }));
      setAutoFillBanner(true);
    } catch { /* silent — AbortError or not found */ }
  }, []);

  useEffect(() => () => { ppLookupAbort.current?.abort(); }, []);

  // ── O.S. No. uniqueness check ─────────────────────────────────────────────
  const handleOsNoChange = (raw: string) => {
    const sanitized = sanitizeInteger(raw);
    setFormData(prev => ({ ...prev, os_no: sanitized }));
    if (!sanitized) { setFieldError('os_no', 'O.S. No. is required.'); return; }
    if (sanitized !== raw) { setFieldError('os_no', 'Digits only.'); return; }
    clearFieldError('os_no');

    clearTimeout(osNoCheckTimer.current);
    osNoCheckTimer.current = setTimeout(async () => {
      try {
        const yr = formData.os_date ? new Date(formData.os_date).getFullYear() : new Date().getFullYear();
        const { data: result } = await api.get('/os/check-os-no', { params: { os_no: sanitized, os_year: yr } });
        if (result.exists) setFieldError('os_no', `O.S. No. ${sanitized}/${yr} already exists!`);
      } catch { /* ignore */ }
    }, 500);
  };

  // ── Validation & submit ───────────────────────────────────────────────────
  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();

    if (!confirmSave) {
      setErrorMsg('Please tick the confirmation checkbox before saving.');
      return;
    }
    setErrorMsg('');
    setFieldErrors({});
    setItemErrors({});

    const errors: Record<string, string> = {};
    const itemErrs: Record<number, Record<string, string>> = {};

    const requireField = (key: string, val: string, label: string) => {
      if (!val.trim()) errors[key] = `${label} is required.`;
    };

    requireField('os_no', formData.os_no, 'O.S. No.');
    requireField('os_date', formData.os_date, 'O.S. Date');
    requireField('booked_by', formData.booked_by, 'Booked By / Batch AIU');
    requireField('flight_no', formData.flight_no, 'Flight No.');
    requireField('pax_name', formData.pax_name, 'Passenger Name');
    requireField('pax_nationality', formData.pax_nationality, 'Nationality');
    requireField('passport_no', formData.passport_no, 'Passport No.');
    requireField('pax_address1', formData.pax_address1, 'Address');
    if (!formData.file_spot) errors['file_spot'] = 'Please select Spot Adjudication or Adjudication vide File.';

    if (!errors.os_no && !/^\d+$/.test(formData.os_no.trim())) {
      errors.os_no = 'O.S. No. must be digits only.';
    }

    if (items.length === 0) {
      errors['items'] = 'At least one seized item is required.';
    }

    items.forEach((itm, idx) => {
      const rowErrors: Record<string, string> = {};
      if (!String(itm.items_desc || '').trim()) rowErrors.items_desc = 'Description is required.';
      if (!String(itm.items_qty || '').trim()) rowErrors.items_qty = 'Quantity required.';
      if (!String(itm.items_value || '').trim() || Number(itm.items_value) === 0) rowErrors.items_value = 'Value required.';
      if (!String(itm.items_duty_type || '').trim()) rowErrors.items_duty_type = 'Duty Type required.';
      if (Object.keys(rowErrors).length > 0) itemErrs[idx] = rowErrors;
    });

    if (Object.keys(errors).length > 0 || Object.keys(itemErrs).length > 0) {
      setFieldErrors(errors);
      setItemErrors(itemErrs);
      setErrorMsg('Please fill all mandatory fields highlighted in red before saving.');
      const firstKey = Object.keys(errors)[0];
      if (firstKey && firstKey !== 'items') {
        const el = document.getElementById(`field-${firstKey}`);
        if (el) { el.scrollIntoView({ behavior: 'smooth', block: 'center' }); (el as any).focus?.(); }
      } else if (Object.keys(itemErrs).length > 0) {
        const firstRow = Number(Object.keys(itemErrs)[0]);
        document.getElementById(`item-row-${firstRow}`)?.scrollIntoView({ behavior: 'smooth', block: 'center' });
      }
      return;
    }

    setIsSubmitting(true);
    try {
      const isDateValid = (d: string) => d && /^\d{4}-\d{2}-\d{2}$/.test(d);
      const payload: any = {
        os_no: formData.os_no,
        os_date: formData.os_date,
        booked_by: formData.booked_by,
        flight_no: formData.flight_no.toUpperCase(),
        pax_name: formData.pax_name.toUpperCase(),
        pax_nationality: formData.pax_nationality.toUpperCase(),
        passport_no: formData.passport_no.toUpperCase(),
        pax_address1: formData.pax_address1.toUpperCase(),
        file_spot: formData.file_spot,
        is_draft: 'N',
        items: items.map((itm, i) => ({
          items_sno: i + 1,
          items_desc: String(itm.items_desc || '').toUpperCase(),
          items_qty: Number(itm.items_qty || 0),
          items_uqc: itm.items_uqc || 'NOS',
          items_value: Number(itm.items_value || 0),
          items_duty_type: itm.items_duty_type || '',
          cumulative_duty_rate: 0,
          value_per_piece: Number(itm.items_value || 0),
        })),
      };

      // Optional fields — only include if non-empty
      if (optionalData.pax_date_of_birth && isDateValid(optionalData.pax_date_of_birth))
        payload.pax_date_of_birth = optionalData.pax_date_of_birth;
      if (optionalData.passport_date && isDateValid(optionalData.passport_date))
        payload.passport_date = optionalData.passport_date;
      if (optionalData.pp_issue_place.trim()) payload.pp_issue_place = optionalData.pp_issue_place.toUpperCase();
      if (optionalData.father_name.trim()) payload.father_name = optionalData.father_name.toUpperCase();
      if (optionalData.residence_at.trim()) payload.residence_at = optionalData.residence_at.toUpperCase();
      if (optionalData.old_passport_no.trim()) payload.old_passport_no = optionalData.old_passport_no.toUpperCase();
      if (optionalData.case_type) payload.case_type = optionalData.case_type;
      if (optionalData.shift) payload.shift = optionalData.shift;
      if (optionalData.supdts_remarks.trim()) payload.supdts_remarks = optionalData.supdts_remarks;

      let savedOsNo: string;
      let savedOsYear: number;
      if (isEditing) {
        await api.put(`/os/${osNo}/${osYear}`, payload);
        savedOsNo = osNo!;
        savedOsYear = Number(osYear);
      } else {
        const res = await api.post('/os/offline', payload);
        savedOsNo = res.data?.os_no || formData.os_no;
        savedOsYear = res.data?.os_year || new Date(formData.os_date).getFullYear();
      }
      setSuccessInfo({ os_no: savedOsNo, os_year: savedOsYear });
      setConfirmSave(false);
    } catch (err: any) {
      let errMsg = err.response?.data?.detail || err.message || 'Failed to save offline case.';
      if (Array.isArray(errMsg)) errMsg = errMsg.map((e: any) => `${e.loc?.join('.')} - ${e.msg}`).join(', ');
      else if (typeof errMsg === 'object') errMsg = JSON.stringify(errMsg);
      setErrorMsg(errMsg);
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleRegisterAnother = () => {
    setSuccessInfo(null);
    setFormData({
      os_no: '', os_date: new Date().toISOString().split('T')[0], booked_by: 'Batch A',
      flight_no: '', pax_name: '', pax_nationality: '', passport_no: '', pax_address1: '', file_spot: '',
    });
    setOptionalData({
      pax_date_of_birth: '', passport_date: '', pp_issue_place: '', father_name: '',
      residence_at: '', old_passport_no: '', case_type: 'Non-Bonafide', shift: 'Day', supdts_remarks: '',
    });
    setItems([{ items_desc: '', items_qty: 1, items_uqc: 'NOS', items_value: 0, items_duty_type: 'Miscellaneous-22' }]);
    setFieldErrors({});
    setItemErrors({});
    setErrorMsg('');
    setAutoFillBanner(false);
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  const totalValue = useMemo(
    () => items.reduce((acc, itm) => acc + Number(itm.items_value || 0), 0),
    [items]
  );

  // ── Excel import handlers ─────────────────────────────────────────────────
  const handleExcelFile = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    e.target.value = '';
    if (file.size > 10 * 1024 * 1024) {
      setErrorMsg('File too large. Please upload an Excel file under 10 MB.');
      return;
    }
    try {
      const buffer = await file.arrayBuffer();
      const wb = XLSX.read(buffer, { type: 'array' });
      const ws = wb.Sheets[wb.SheetNames[0]];
      if (!ws) { setErrorMsg('Excel file appears empty or has no sheets.'); return; }
      const data: any[][] = XLSX.utils.sheet_to_json(ws, { header: 1, defval: '' });
      const rows = parseExcelRows(data);
      setParsedRows(rows);
      setRowRfRefOverrides({});
      setRowItemOverrides({});
      setExpandedItemEdit(new Set());
      setImportStatus(null);
    } catch {
      setErrorMsg('Could not read the file. Please upload a valid .xlsx, .xls, or .csv file.');
    }
  };

  const handleExcelImport = async () => {
    setImportLoading(true);
    setImportStatus(null);
    const payload = parsedRows.map(row => {
      let { rf_amount, ref_amount, confiscated_value } = row;
      if (row.rfRefType === 'ambiguous') {
        const chosen = rowRfRefOverrides[row.sno] || 'rf';
        rf_amount = 0; ref_amount = 0; confiscated_value = 0;
        if (chosen === 'rf')          rf_amount = row.rfRefValue;
        else if (chosen === 'ref')    ref_amount = row.rfRefValue;
        else                          confiscated_value = row.rfRefValue;
      }
      // 'none' rows pass through with all-zero amounts (AIU case — outcome added later)
      // Use manually confirmed items if user reviewed them, otherwise use parsed items
      const items = rowItemOverrides[row.sno] || row.items;
      return { ...row, rf_amount, ref_amount, confiscated_value, items };
    });
    try {
      const res = await api.post('/os/offline/bulk-import', payload);
      setImportStatus(res.data);
    } catch (err: any) {
      const rawDetail = err.response?.data?.detail ?? err.message ?? 'Import failed';
      // Prevent React error #31: ensure detail is always a string, never an object
      const detail = Array.isArray(rawDetail)
        ? rawDetail.map((e: any) => e.msg ?? JSON.stringify(e)).join('; ')
        : typeof rawDetail === 'object'
          ? JSON.stringify(rawDetail)
          : String(rawDetail);
      setImportStatus({ imported: 0, skipped: 0, failed: [{ error: detail }], total: parsedRows.length });
    } finally {
      setImportLoading(false);
    }
  };

  // ── Success screen ────────────────────────────────────────────────────────
  if (successInfo) {
    return (
      <div className="space-y-4 w-full pb-20">
        <div className="flex items-center bg-white px-4 py-3 border-b border-slate-200 rounded-xl border">
          <button onClick={() => navigate('/sdo')} className="p-2 bg-slate-50 border border-slate-200 rounded-md hover:bg-slate-100 transition-colors mr-4">
            <ArrowLeft size={20} className="text-slate-600" />
          </button>
          <h1 className="text-2xl font-bold text-slate-800 tracking-tight">Register Offline Adjudication Case</h1>
        </div>
        <div className="bg-white rounded-xl border border-green-200 p-10 text-center space-y-4 max-w-xl mx-auto mt-8">
          <CheckCircle size={48} className="text-green-500 mx-auto" />
          <h2 className="text-xl font-bold text-slate-800">Case Registered Successfully</h2>
          <p className="text-slate-600 text-sm">
            Offline adjudication case{' '}
            <span className="font-bold text-blue-700">O.S. {successInfo.os_no}/{successInfo.os_year}</span>{' '}
            has been saved and is pending completion by the adjudication officer.
          </p>
          <div className="flex gap-3 justify-center pt-2">
            <button
              onClick={handleRegisterAnother}
              className="px-5 py-2 bg-blue-700 text-white font-semibold rounded-lg hover:bg-blue-600 transition-colors text-sm"
            >
              Register Another
            </button>
            <button
              onClick={() => navigate('/sdo')}
              className="px-5 py-2 border border-slate-300 text-slate-700 font-semibold rounded-lg hover:bg-slate-50 transition-colors text-sm"
            >
              Back to SDO
            </button>
          </div>
        </div>
      </div>
    );
  }

  if (editLoading) {
    return (
      <div className="flex items-center justify-center h-64 text-slate-500">
        <div className="flex flex-col items-center gap-3">
          <div className="w-8 h-8 border-4 border-blue-200 border-t-blue-600 rounded-full animate-spin"></div>
          <span className="font-medium text-sm">Loading case details...</span>
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-4 w-full pb-20">
      {descDatalist}

      {/* Header */}
      <div className="flex justify-between items-center bg-white px-4 py-3 border-b border-slate-200 rounded-xl border">
        <div className="flex items-center space-x-4">
          <button onClick={() => navigate(isEditing ? '/sdo/offence' : '/sdo')} className="p-2 bg-slate-50 border border-slate-200 rounded-md hover:bg-slate-100 transition-colors">
            <ArrowLeft size={20} className="text-slate-600" />
          </button>
          <div>
            <h1 className="text-2xl font-bold text-slate-800 tracking-tight">
              {isEditing ? `Edit Offline Case — O.S. ${osNo}/${osYear}` : 'Register Offline Adjudication Case'}
            </h1>
            <p className="text-slate-500 text-sm mt-0.5">
              {isEditing
                ? 'Update the case details and seized goods below.'
                : 'Fill in the case details and seized goods. Officer details will be added by the adjudication module.'}
            </p>
          </div>
        </div>
        <span className="bg-blue-100 text-blue-800 border border-blue-300 px-3 py-1.5 rounded-lg text-xs font-bold">
          OFFLINE ADJ
        </span>
      </div>

      {/* Input mode toggle — only shown when creating (not editing) */}
      {!isEditing && (
        <div className="flex gap-3">
          <button
            type="button"
            onClick={() => { setInputMode('manual'); setImportStatus(null); }}
            className={`flex items-center gap-2 px-5 py-3.5 rounded-xl border-2 font-semibold text-sm transition-all ${inputMode === 'manual' ? 'border-blue-500 bg-blue-50 text-blue-700' : 'border-slate-200 bg-white text-slate-600 hover:border-slate-300'}`}
          >
            <PenLine size={16} /> Add Data Manually
          </button>
          <button
            type="button"
            onClick={() => { setInputMode('excel'); setImportStatus(null); }}
            className={`flex items-center gap-2 px-5 py-3.5 rounded-xl border-2 font-semibold text-sm transition-all ${inputMode === 'excel' ? 'border-green-500 bg-green-50 text-green-700' : 'border-slate-200 bg-white text-slate-600 hover:border-slate-300'}`}
          >
            <Table2 size={16} /> Add Data through Excel
          </button>
        </div>
      )}

      {inputMode === 'manual' && (<>
      {/* Error banner */}
      {errorMsg && (
        <div className="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded-lg flex items-start">
          <AlertCircle className="shrink-0 mr-3 mt-0.5" size={20} />
          <div>
            <h4 className="font-bold text-sm">Validation Error</h4>
            <p className="text-sm">{errorMsg}</p>
            {(Object.keys(fieldErrors).length > 0 || Object.keys(itemErrors).length > 0) && (
              <ul className="list-disc pl-4 text-xs mt-2 space-y-1">
                {Object.entries(fieldErrors).map(([k, m]) => <li key={k}>{m}</li>)}
                {Object.entries(itemErrors).flatMap(([idxStr, errs]) =>
                  Object.entries(errs).map(([k, m]) => (
                    <li key={`item-${idxStr}-${k}`}>Seized Item {Number(idxStr) + 1}: {m}</li>
                  ))
                )}
              </ul>
            )}
          </div>
        </div>
      )}

      {/* Auto-fill banner */}
      {autoFillBanner && (
        <div className="bg-blue-50 border border-blue-200 text-blue-700 px-4 py-3 rounded-lg flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Info size={16} className="shrink-0" />
            <span className="text-sm font-medium">Passenger details auto-filled from previous case record</span>
          </div>
          <button onClick={() => setAutoFillBanner(false)} className="text-blue-400 hover:text-blue-600 text-lg leading-none font-bold px-2">&times;</button>
        </div>
      )}

      <form
        onSubmit={handleSubmit}
        onKeyDown={e => { if (e.key === 'Enter' && (e.target as HTMLElement).tagName !== 'BUTTON') e.preventDefault(); }}
        className="space-y-6"
      >
        {/* ── Top Details Grid ─────────────────────────────────────────────── */}
        <div className="grid grid-cols-1 xl:grid-cols-2 gap-6">

          {/* Case Registration Details */}
          <div className="bg-white p-5 rounded-xl border border-slate-200">
            <h2 className="text-sm font-bold text-slate-800 uppercase tracking-wider mb-4 border-b border-slate-100 pb-2 flex items-center">
              <FileText className="mr-2 text-blue-600" size={16} /> Case Registration Details
            </h2>
            <div className="space-y-3">
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">
                    O.S. No. <span className="text-red-500">*</span>
                  </label>
                  <input
                    id="field-os_no"
                    type="text"
                    inputMode="numeric"
                    className={`w-full px-3 py-2 bg-slate-50 border ${fieldErrors.os_no ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-blue-500'} rounded focus:ring-2 text-sm`}
                    value={formData.os_no}
                    onChange={e => handleOsNoChange(e.target.value)}
                  />
                  {fieldErrors.os_no && <p className="mt-1 text-xs font-semibold text-red-600">{fieldErrors.os_no}</p>}
                </div>
                <div>
                  <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">
                    O.S. Date <span className="text-red-500">*</span>
                  </label>
                  <DatePicker
                    id="field-os_date"
                    value={formData.os_date}
                    onChange={isoDate => setFormData(prev => ({ ...prev, os_date: isoDate }))}
                    inputClassName="w-full px-3 py-2 bg-slate-50 border border-slate-300 focus:ring-blue-500 rounded focus:ring-2 text-sm"
                    error={!!fieldErrors.os_date}
                  />
                  {fieldErrors.os_date && <p className="mt-1 text-xs font-semibold text-red-600">{fieldErrors.os_date}</p>}
                </div>
              </div>
              <div>
                <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">
                  Booked By / Batch AIU <span className="text-red-500">*</span>
                </label>
                <select
                  id="field-booked_by"
                  className={`w-full px-3 py-2 bg-slate-50 border ${fieldErrors.booked_by ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-blue-500'} rounded focus:ring-2 text-sm`}
                  value={formData.booked_by}
                  onChange={e => setFormData(prev => ({ ...prev, booked_by: e.target.value }))}
                >
                  <option>Batch A</option><option>Batch B</option><option>Batch C</option><option>Batch D</option>
                  <option>AIU A</option><option>AIU B</option><option>AIU C</option><option>AIU D</option>
                </select>
                {fieldErrors.booked_by && <p className="mt-1 text-xs font-semibold text-red-600">{fieldErrors.booked_by}</p>}
              </div>
              <div>
                <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">
                  Flight No. <span className="text-red-500">*</span>
                </label>
                <input
                  id="field-flight_no"
                  type="text"
                  className={`w-full px-3 py-2 bg-slate-50 border ${fieldErrors.flight_no ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-blue-500'} rounded focus:ring-2 text-sm uppercase font-medium`}
                  value={formData.flight_no}
                  onChange={e => { setFormData(prev => ({ ...prev, flight_no: e.target.value.toUpperCase() })); clearFieldError('flight_no'); }}
                />
                {fieldErrors.flight_no && <p className="mt-1 text-xs font-semibold text-red-600">{fieldErrors.flight_no}</p>}
              </div>
            </div>
          </div>

          {/* Passenger & Passport */}
          <div className="bg-white p-5 rounded-xl border border-slate-200 relative overflow-hidden">
            <div className="absolute top-0 left-0 w-1 h-full bg-blue-500"></div>
            <h2 className="text-sm font-bold text-slate-800 uppercase tracking-wider mb-4 border-b border-slate-100 pb-2 flex items-center">
              <User className="mr-2 text-blue-500" size={16} /> Passenger &amp; Passport Information
            </h2>
            <div className="space-y-3">
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">
                    Passenger Name <span className="text-red-500">*</span>
                  </label>
                  <input
                    id="field-pax_name"
                    type="text"
                    className={`w-full px-3 py-1.5 border ${fieldErrors.pax_name ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-blue-500'} rounded text-sm focus:ring-2 uppercase font-medium`}
                    value={formData.pax_name}
                    onChange={e => { setFormData(prev => ({ ...prev, pax_name: e.target.value.toUpperCase() })); clearFieldError('pax_name'); }}
                  />
                  {fieldErrors.pax_name && <p className="mt-1 text-xs font-semibold text-red-600">{fieldErrors.pax_name}</p>}
                </div>
                <div>
                  <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">
                    Nationality <span className="text-red-500">*</span>
                  </label>
                  <input
                    id="field-pax_nationality"
                    type="text"
                    className={`w-full px-3 py-1.5 border ${fieldErrors.pax_nationality ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-blue-500'} rounded text-sm focus:ring-2 uppercase`}
                    value={formData.pax_nationality}
                    onChange={e => { setFormData(prev => ({ ...prev, pax_nationality: e.target.value.toUpperCase() })); clearFieldError('pax_nationality'); }}
                  />
                  {fieldErrors.pax_nationality && <p className="mt-1 text-xs font-semibold text-red-600">{fieldErrors.pax_nationality}</p>}
                </div>
              </div>
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">
                    Passport No. <span className="text-red-500">*</span>
                  </label>
                  <input
                    id="field-passport_no"
                    type="text"
                    className={`w-full px-3 py-1.5 border ${fieldErrors.passport_no ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-blue-500'} rounded text-sm focus:ring-2 uppercase font-bold text-slate-800`}
                    value={formData.passport_no}
                    onChange={e => { setFormData(prev => ({ ...prev, passport_no: e.target.value.toUpperCase() })); clearFieldError('passport_no'); }}
                    onBlur={e => handlePassportBlur(e.target.value)}
                  />
                  {fieldErrors.passport_no && <p className="mt-1 text-xs font-semibold text-red-600">{fieldErrors.passport_no}</p>}
                </div>
                <div className="flex items-end">
                  <p className="text-[10px] text-slate-400 italic pb-1.5">Blur to auto-fill passenger details if passport is in COPS</p>
                </div>
              </div>
              <div>
                <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">
                  Address <span className="text-red-500">*</span>
                </label>
                <textarea
                  id="field-pax_address1"
                  rows={2}
                  className={`w-full px-3 py-1.5 border ${fieldErrors.pax_address1 ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-blue-500'} rounded text-sm focus:ring-2 uppercase resize-none`}
                  value={formData.pax_address1}
                  onChange={e => { setFormData(prev => ({ ...prev, pax_address1: e.target.value.toUpperCase() })); clearFieldError('pax_address1'); }}
                  placeholder="Full residential address"
                />
                {fieldErrors.pax_address1 && <p className="mt-1 text-xs font-semibold text-red-600">{fieldErrors.pax_address1}</p>}
              </div>
            </div>
          </div>
        </div>

        {/* ── Additional Details (collapsible) ─────────────────────────────── */}
        <div className="bg-white rounded-xl border border-slate-200 overflow-hidden">
          <button
            type="button"
            onClick={() => setAdditionalOpen(o => !o)}
            className="w-full flex items-center justify-between px-5 py-3.5 text-left hover:bg-slate-50 transition-colors"
          >
            <span className="text-sm font-bold text-slate-700 uppercase tracking-wider flex items-center">
              <Plane className="mr-2 text-slate-400" size={15} /> Additional Details
              <span className="ml-2 text-xs font-normal text-slate-400 normal-case">(optional)</span>
            </span>
            <span className="text-slate-400 text-lg leading-none">{additionalOpen ? '▲' : '▼'}</span>
          </button>
          {additionalOpen && (
            <div className="px-5 pb-5 pt-2 border-t border-slate-100">
              <div className="grid grid-cols-2 md:grid-cols-3 xl:grid-cols-4 gap-4">
                <div>
                  <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">DOB</label>
                  <DatePicker
                    value={optionalData.pax_date_of_birth}
                    onChange={d => setOptionalData(prev => ({ ...prev, pax_date_of_birth: d }))}
                    inputClassName="w-full px-3 py-2 bg-slate-50 border border-slate-300 focus:ring-blue-500 rounded focus:ring-2 text-sm"
                  />
                </div>
                <div>
                  <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">Passport Date / Expiry</label>
                  <DatePicker
                    value={optionalData.passport_date}
                    onChange={d => setOptionalData(prev => ({ ...prev, passport_date: d }))}
                    inputClassName="w-full px-3 py-2 bg-slate-50 border border-slate-300 focus:ring-blue-500 rounded focus:ring-2 text-sm"
                  />
                </div>
                <div>
                  <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">Place of Issue</label>
                  <input
                    type="text"
                    className="w-full px-3 py-2 bg-slate-50 border border-slate-300 rounded focus:ring-2 focus:ring-blue-500 text-sm uppercase"
                    value={optionalData.pp_issue_place}
                    onChange={e => setOptionalData(prev => ({ ...prev, pp_issue_place: e.target.value.toUpperCase() }))}
                  />
                </div>
                <div>
                  <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">Father's Name</label>
                  <input
                    type="text"
                    className="w-full px-3 py-2 bg-slate-50 border border-slate-300 rounded focus:ring-2 focus:ring-blue-500 text-sm uppercase"
                    value={optionalData.father_name}
                    onChange={e => setOptionalData(prev => ({ ...prev, father_name: e.target.value.toUpperCase() }))}
                  />
                </div>
                <div>
                  <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">Normal Resident At</label>
                  <input
                    type="text"
                    className="w-full px-3 py-2 bg-slate-50 border border-slate-300 rounded focus:ring-2 focus:ring-blue-500 text-sm uppercase"
                    value={optionalData.residence_at}
                    onChange={e => setOptionalData(prev => ({ ...prev, residence_at: e.target.value.toUpperCase() }))}
                    placeholder="e.g. INDIA, ABROAD"
                  />
                </div>
                <div>
                  <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">Old Passport Nos</label>
                  <input
                    type="text"
                    className="w-full px-3 py-2 bg-slate-50 border border-slate-300 rounded focus:ring-2 focus:ring-blue-500 text-sm uppercase"
                    value={optionalData.old_passport_no}
                    onChange={e => setOptionalData(prev => ({ ...prev, old_passport_no: e.target.value.toUpperCase() }))}
                    placeholder="Separate with ;"
                  />
                </div>
                <div>
                  <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">Case Type</label>
                  <select
                    className="w-full px-3 py-2 bg-slate-50 border border-slate-300 rounded focus:ring-2 focus:ring-blue-500 text-sm"
                    value={optionalData.case_type}
                    onChange={e => setOptionalData(prev => ({ ...prev, case_type: e.target.value }))}
                  >
                    <option>Non-Bonafide</option>
                    <option>Mis-Declaration</option>
                    <option>Concealment</option>
                    <option>Trade Goods</option>
                    <option>Unclaimed</option>
                    <option>Export Case</option>
                  </select>
                </div>
                <div>
                  <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">Shift</label>
                  <select
                    className="w-full px-3 py-2 bg-slate-50 border border-slate-300 rounded focus:ring-2 focus:ring-blue-500 text-sm"
                    value={optionalData.shift}
                    onChange={e => setOptionalData(prev => ({ ...prev, shift: e.target.value }))}
                  >
                    <option>Day</option>
                    <option>Night</option>
                  </select>
                </div>
                <div className="col-span-2 md:col-span-3 xl:col-span-4">
                  <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">Supdt's Remarks</label>
                  <textarea
                    rows={3}
                    className="w-full px-3 py-2 bg-slate-50 border border-slate-300 rounded focus:ring-2 focus:ring-blue-500 text-sm resize-none"
                    value={optionalData.supdts_remarks}
                    onChange={e => setOptionalData(prev => ({ ...prev, supdts_remarks: e.target.value }))}
                    maxLength={1500}
                    placeholder="Optional remarks..."
                  />
                  <p className="text-right text-[10px] text-slate-400 mt-0.5">{optionalData.supdts_remarks.length}/1500</p>
                </div>
              </div>
            </div>
          )}
        </div>

        {/* ── Seized Goods Registration ─────────────────────────────────────── */}
        <div className="bg-white rounded-xl border border-slate-200 overflow-hidden">
          <div className="p-4 border-b border-slate-200 bg-slate-50 flex justify-between items-center">
            <h2 className="text-sm font-bold text-slate-800 uppercase flex items-center tracking-wider">
              <FileText className="mr-2 text-orange-500" size={16} /> Seized Goods Registration
              <span className="ml-2 text-red-500 text-xs font-bold">*</span>
            </h2>
            <button
              type="button"
              onClick={() => setItems(prev => [...prev, {
                items_desc: '', items_qty: 1, items_uqc: 'NOS', items_value: 0, items_duty_type: 'Miscellaneous-22'
              }])}
              className="text-xs px-3 py-1.5 bg-white text-orange-700 hover:bg-orange-50 border border-orange-200 rounded font-bold flex items-center transition-colors uppercase tracking-wider"
            >
              <Plus size={14} className="mr-1" /> Add Item
            </button>
          </div>
          {fieldErrors.items && (
            <p className="px-4 py-2 text-xs font-semibold text-red-600 bg-red-50 border-b border-red-100">{fieldErrors.items}</p>
          )}
          <div className="overflow-auto">
            <table className="w-full text-sm text-left whitespace-nowrap">
              <thead className="text-[10px] text-slate-500 uppercase bg-slate-100 border-b border-slate-200 tracking-wider">
                <tr>
                  <th className="px-3 py-2 font-bold text-center w-10">S.No</th>
                  <th className="px-3 py-2 font-bold w-56">Description of Goods</th>
                  <th className="px-3 py-2 font-bold w-36 text-center">Quantity &amp; Unit</th>
                  <th className="px-3 py-2 font-bold w-28 text-right">Value (₹)</th>
                  <th className="px-3 py-2 font-bold w-40">Duty Type</th>
                  <th className="px-3 py-2 font-bold w-10"></th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-100 bg-white">
                {items.length === 0 ? (
                  <tr>
                    <td colSpan={6} className="text-center py-8 text-slate-400">
                      Click "Add Item" to register seized goods.
                    </td>
                  </tr>
                ) : (
                  items.map((itm, idx) => (
                    <SimpleItemRow
                      key={idx}
                      itm={itm}
                      idx={idx}
                      rowErrors={itemErrors[idx]}
                      updateItem={updateItem}
                      onRemove={onRemove}
                      onDescBlur={onDescBlur}
                      descDatalistId="offline-item-desc-datalist"
                    />
                  ))
                )}
              </tbody>
              {items.length > 0 && (
                <tfoot className="bg-slate-50 border-t border-slate-200">
                  <tr>
                    <td colSpan={3} className="px-3 py-2 text-xs font-bold text-slate-600 uppercase text-right">Total Value:</td>
                    <td className="px-2 py-2 text-right font-bold text-sm text-slate-800">
                      ₹ {totalValue.toLocaleString('en-IN', { maximumFractionDigits: 2 })}
                    </td>
                    <td colSpan={2} />
                  </tr>
                </tfoot>
              )}
            </table>
          </div>
        </div>

        {/* ── Adjudication Type ─────────────────────────────────────────────── */}
        <div className={`bg-white rounded-xl border-2 ${fieldErrors.file_spot ? 'border-red-400' : 'border-slate-200'} p-5`}>
          <p className="text-sm font-bold text-slate-700 uppercase tracking-wider mb-3 flex items-center">
            <FileText className="mr-2 text-slate-400" size={15} />
            Adjudication Type <span className="text-red-500 ml-1">*</span>
            <span className="ml-2 text-xs font-normal text-slate-400 normal-case">(mandatory)</span>
          </p>
          <div className="flex flex-col sm:flex-row gap-4">
            <label className={`flex items-center gap-3 cursor-pointer px-5 py-3.5 rounded-xl border-2 transition-all select-none flex-1 ${formData.file_spot === 'Spot' ? 'border-blue-500 bg-blue-50 text-blue-800' : 'border-slate-200 bg-slate-50 text-slate-700 hover:border-slate-300'}`}>
              <input
                type="radio"
                name="file_spot"
                value="Spot"
                checked={formData.file_spot === 'Spot'}
                onChange={() => { setFormData(prev => ({ ...prev, file_spot: 'Spot' })); clearFieldError('file_spot'); }}
                className="w-4 h-4 accent-blue-600"
              />
              <div>
                <div className="font-bold text-sm">Spot Adjudication</div>
                <div className="text-xs text-slate-500 mt-0.5">Adjudication done at the spot / on the day of seizure</div>
              </div>
            </label>
            <label className={`flex items-center gap-3 cursor-pointer px-5 py-3.5 rounded-xl border-2 transition-all select-none flex-1 ${formData.file_spot === 'File' ? 'border-purple-500 bg-purple-50 text-purple-800' : 'border-slate-200 bg-slate-50 text-slate-700 hover:border-slate-300'}`}>
              <input
                type="radio"
                name="file_spot"
                value="File"
                checked={formData.file_spot === 'File'}
                onChange={() => { setFormData(prev => ({ ...prev, file_spot: 'File' })); clearFieldError('file_spot'); }}
                className="w-4 h-4 accent-purple-600"
              />
              <div>
                <div className="font-bold text-sm">Adjudication vide File</div>
                <div className="text-xs text-slate-500 mt-0.5">Adjudication done through file / after issuing show cause notice</div>
              </div>
            </label>
          </div>
          {fieldErrors.file_spot && (
            <p className="mt-2 text-xs font-semibold text-red-600 flex items-center gap-1">
              <AlertCircle size={13} /> {fieldErrors.file_spot}
            </p>
          )}
        </div>

        {/* ── Confirmation + Submit ──────────────────────────────────────── */}
        <div className="border-t border-slate-200 pt-4 mt-2 space-y-3">
          <label className="flex items-center gap-2.5 cursor-pointer select-none">
            <input
              type="checkbox"
              className="w-4 h-4 rounded border-slate-300 text-blue-600 focus:ring-blue-500"
              checked={confirmSave}
              onChange={e => setConfirmSave(e.target.checked)}
            />
            <span className="text-sm font-medium text-slate-700">
              I confirm the above details are correct and wish to save.
            </span>
          </label>
          <div className="flex items-center justify-end gap-3">
            <button
              type="button"
              onClick={() => navigate(isEditing ? '/sdo/offence' : '/sdo')}
              className="px-5 py-2 border border-slate-300 bg-white text-slate-700 font-semibold rounded-lg hover:bg-slate-50 transition-colors text-sm"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={isSubmitting || !confirmSave}
              className="px-6 py-2 bg-blue-700 text-white font-semibold rounded-lg hover:bg-blue-600 transition-colors text-sm disabled:opacity-60 flex items-center gap-2"
            >
              {isSubmitting ? (
                <>
                  <span className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin"></span>
                  Saving...
                </>
              ) : isEditing ? 'Update Case' : 'Save Offline Case'}
            </button>
          </div>
        </div>
      </form>
      </>)}

      {/* ── Excel import mode ──────────────────────────────────────────────── */}
      {inputMode === 'excel' && (
        <div className="space-y-4">

          {/* File picker card */}
          <div className="bg-white rounded-xl border border-slate-200 p-6">
            <h2 className="text-sm font-bold text-slate-800 uppercase tracking-wider mb-3 flex items-center">
              <Upload className="mr-2 text-green-600" size={16} /> Import from Monthly Report Excel
            </h2>
            <p className="text-xs text-slate-500 mb-4">
              Upload the monthly report Excel / CSV file (.xlsx / .xls / .csv). All rows are parsed and only cases
              not already in the database are imported — duplicates are silently skipped.
            </p>
            <label className="flex flex-col items-center justify-center w-full h-32 border-2 border-dashed border-slate-300 rounded-xl cursor-pointer hover:border-green-400 hover:bg-green-50 transition-all">
              <Upload size={24} className="text-slate-400 mb-2" />
              <span className="text-sm font-medium text-slate-600">Click to select Excel file</span>
              <span className="text-xs text-slate-400 mt-1">.xlsx, .xls or .csv</span>
              <input type="file" accept=".xlsx,.xls,.csv" className="hidden" onChange={handleExcelFile} />
            </label>
          </div>

          {/* Parsed rows preview */}
          {parsedRows.length > 0 && !importStatus && (
            <div className="bg-white rounded-xl border border-slate-200 overflow-hidden">
              <div className="p-4 border-b border-slate-200 bg-slate-50 flex justify-between items-center">
                <h2 className="text-sm font-bold text-slate-800 uppercase flex items-center tracking-wider">
                  <Table2 className="mr-2 text-green-600" size={16} /> Preview — {parsedRows.length} rows parsed
                </h2>
                <span className="text-xs text-slate-500">
                  {parsedRows.filter(r => r.warnings.length > 0).length} rows with warnings
                </span>
              </div>
              <div className="overflow-auto max-h-[480px]">
                <table className="w-full text-xs text-left whitespace-nowrap">
                  <thead className="text-[10px] text-slate-500 uppercase bg-slate-100 border-b border-slate-200 tracking-wider sticky top-0 z-10">
                    <tr>
                      <th className="px-2 py-2">#</th>
                      <th className="px-2 py-2">OS No.</th>
                      <th className="px-2 py-2">Date</th>
                      <th className="px-2 py-2">Passenger</th>
                      <th className="px-2 py-2">Passport</th>
                      <th className="px-2 py-2">Flight</th>
                      <th className="px-2 py-2">Items</th>
                      <th className="px-2 py-2 text-right">Value (₹)</th>
                      <th className="px-2 py-2">RF/REF Type</th>
                      <th className="px-2 py-2 text-right">RF/REF (₹)</th>
                      <th className="px-2 py-2 text-right">Penalty (₹)</th>
                      <th className="px-2 py-2">Adj. Officer</th>
                      <th className="px-2 py-2">Warnings</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-slate-100">
                    {parsedRows.map((row, i) => {
                      const confirmedItems = rowItemOverrides[row.sno];
                      const displayItems = confirmedItems || row.items;
                      const itemSummary = displayItems.map(it => it.items_desc).join('; ');
                      const isEditingItems = expandedItemEdit.has(row.sno);
                      return (
                        <Fragment key={row.sno}>
                          <tr className={row.warnings.length > 0 ? 'bg-amber-50' : 'hover:bg-slate-50'}>
                            <td className="px-2 py-1.5 text-slate-400 font-medium">{i + 1}</td>
                            <td className="px-2 py-1.5 font-bold text-slate-800">{row.os_no}/{row.os_year}</td>
                            <td className="px-2 py-1.5 text-slate-600">{row.os_date}</td>
                            <td className="px-2 py-1.5 max-w-[120px] truncate text-slate-700" title={row.pax_name}>{row.pax_name}</td>
                            <td className="px-2 py-1.5 font-mono text-slate-600">{row.passport_no}</td>
                            <td className="px-2 py-1.5 text-slate-600">{row.flight_no}</td>
                            <td className="px-2 py-1.5 max-w-[180px] text-slate-600">
                              <div className="flex items-start gap-1">
                                <div className="flex-1 min-w-0">
                                  {row.needsManualItems && !confirmedItems && (
                                    <span className="text-amber-600 text-[10px] font-semibold block">⚠ needs review</span>
                                  )}
                                  {confirmedItems && (
                                    <span className="text-green-700 text-[10px] font-semibold block">✓ confirmed</span>
                                  )}
                                  <span className="text-[10px] text-slate-500 truncate block" title={itemSummary}>
                                    {displayItems.length} item{displayItems.length !== 1 ? 's' : ''}
                                    {displayItems.length > 0 && `: ${displayItems[0].items_desc.slice(0, 22)}${displayItems.length > 1 || displayItems[0].items_desc.length > 22 ? '…' : ''}`}
                                  </span>
                                  {displayItems.length === 1 && (
                                    <span className="text-[10px] text-slate-400">
                                      ×{displayItems[0].items_qty} {displayItems[0].items_uqc}
                                    </span>
                                  )}
                                </div>
                                <button
                                  type="button"
                                  title="Edit items"
                                  onClick={() => setExpandedItemEdit(prev => { const n = new Set(prev); if (n.has(row.sno)) n.delete(row.sno); else n.add(row.sno); return n; })}
                                  className={`flex-shrink-0 p-0.5 rounded transition-colors ${isEditingItems ? 'text-blue-600 bg-blue-100' : 'text-slate-400 hover:text-blue-600 hover:bg-blue-50'}`}
                                >
                                  <PenLine size={11} />
                                </button>
                              </div>
                            </td>
                            <td className="px-2 py-1.5 text-right font-medium">{row.total_items_value.toLocaleString('en-IN')}</td>
                            <td className="px-2 py-1.5">
                              {row.rfRefType === 'none' ? (
                                <span className="px-1.5 py-0.5 rounded text-[10px] font-bold uppercase bg-gray-100 text-gray-500">NIL</span>
                              ) : row.rfRefType === 'ambiguous' ? (
                                <select
                                  className="px-1 py-0.5 border border-amber-400 rounded text-[10px] bg-amber-50 focus:outline-none focus:ring-1 focus:ring-amber-500"
                                  value={rowRfRefOverrides[row.sno] || ''}
                                  onChange={e => setRowRfRefOverrides(prev => ({ ...prev, [row.sno]: e.target.value as 'rf' | 'ref' | 'confiscated' }))}
                                >
                                  <option value="">— select —</option>
                                  <option value="rf">RF (Confiscation)</option>
                                  <option value="ref">REF (Re-Export)</option>
                                  <option value="confiscated">Abs. Confiscation</option>
                                </select>
                              ) : (
                                <span className={`px-1.5 py-0.5 rounded text-[10px] font-bold uppercase ${
                                  row.rfRefType === 'rf' ? 'bg-red-100 text-red-700'
                                  : row.rfRefType === 'ref' ? 'bg-blue-100 text-blue-700'
                                  : 'bg-purple-100 text-purple-700'
                                }`}>{row.rfRefType}</span>
                              )}
                            </td>
                            <td className="px-2 py-1.5 text-right font-medium">{row.rfRefValue.toLocaleString('en-IN')}</td>
                            <td className="px-2 py-1.5 text-right font-medium">
                              {row.pp_amount > 0 ? row.pp_amount.toLocaleString('en-IN') : <span className="text-slate-300">—</span>}
                            </td>
                            <td className="px-2 py-1.5 max-w-[120px] truncate text-slate-600" title={row.adj_offr_name}>{row.adj_offr_name}</td>
                            <td className="px-2 py-1.5">
                              {row.warnings.length > 0 && (
                                <span className="text-amber-700 text-[10px]" title={row.warnings.join('\n')}>
                                  ⚠ {row.warnings.join('; ')}
                                </span>
                              )}
                            </td>
                          </tr>
                          {isEditingItems && (
                            <tr>
                              <td colSpan={13} className="px-3 py-2 bg-blue-50/40">
                                <ItemEditPanel
                                  initialItems={confirmedItems || row.items}
                                  totalValue={row.total_items_value}
                                  penalty={row.pp_amount}
                                  onConfirm={items => {
                                    setRowItemOverrides(prev => ({ ...prev, [row.sno]: items }));
                                    setExpandedItemEdit(prev => { const n = new Set(prev); n.delete(row.sno); return n; });
                                  }}
                                  onCancel={() => setExpandedItemEdit(prev => { const n = new Set(prev); n.delete(row.sno); return n; })}
                                />
                              </td>
                            </tr>
                          )}
                        </Fragment>
                      );
                    })}
                  </tbody>
                </table>
              </div>

              {/* Import action bar */}
              <div className="p-4 border-t border-slate-200 bg-slate-50 flex items-center justify-between gap-4">
                <div className="text-xs text-slate-500">
                  {parsedRows.filter(r => r.rfRefType === 'ambiguous' && !rowRfRefOverrides[r.sno]).length > 0 && (
                    <span className="text-amber-700 font-medium">
                      ⚠ {parsedRows.filter(r => r.rfRefType === 'ambiguous' && !rowRfRefOverrides[r.sno]).length} rows still need RF/REF type selected
                    </span>
                  )}
                </div>
                <button
                  type="button"
                  disabled={importLoading || parsedRows.length === 0
                    || parsedRows.some(r => r.rfRefType === 'ambiguous' && !rowRfRefOverrides[r.sno])
                    || parsedRows.some(r => r.needsManualItems && !rowItemOverrides[r.sno])}
                  onClick={handleExcelImport}
                  className="flex items-center gap-2 px-6 py-2.5 bg-green-700 text-white font-semibold rounded-lg hover:bg-green-600 transition-colors text-sm disabled:opacity-60"
                >
                  {importLoading ? (
                    <>
                      <span className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                      Importing…
                    </>
                  ) : (
                    <>
                      <Upload size={15} /> Import {parsedRows.length} Cases
                    </>
                  )}
                </button>
              </div>
            </div>
          )}

          {/* Import result */}
          {importStatus && (
            <div className={`rounded-xl border p-6 ${importStatus.failed.length > 0 ? 'bg-amber-50 border-amber-200' : 'bg-green-50 border-green-200'}`}>
              <h3 className="font-bold text-sm text-slate-800 mb-4 flex items-center gap-2">
                <CheckCircle size={16} className="text-green-600" /> Import Complete
              </h3>
              <div className="flex gap-8 text-sm mb-4">
                <div className="text-center">
                  <div className="text-3xl font-bold text-green-700">{importStatus.imported}</div>
                  <div className="text-xs text-slate-500 mt-0.5">Imported</div>
                </div>
                <div className="text-center">
                  <div className="text-3xl font-bold text-amber-600">{importStatus.skipped}</div>
                  <div className="text-xs text-slate-500 mt-0.5">Skipped (already in DB)</div>
                </div>
                <div className="text-center">
                  <div className="text-3xl font-bold text-red-600">{importStatus.failed.length}</div>
                  <div className="text-xs text-slate-500 mt-0.5">Failed</div>
                </div>
              </div>
              {importStatus.failed.length > 0 && (
                <div className="mb-4 space-y-1">
                  <p className="text-xs font-semibold text-red-700">Failed rows:</p>
                  {importStatus.failed.map((f: any, i: number) => (
                    <p key={i} className="text-xs text-red-600 font-mono bg-red-50 px-2 py-1 rounded">
                      {f.os_no ? `OS ${f.os_no}/${f.os_year}: ` : ''}{typeof f.error === 'string' ? f.error : JSON.stringify(f.error)}
                    </p>
                  ))}
                </div>
              )}
              <div className="flex gap-3">
                <button
                  type="button"
                  onClick={() => { setParsedRows([]); setImportStatus(null); setRowRfRefOverrides({}); }}
                  className="px-4 py-2 text-sm font-semibold bg-white border border-slate-300 text-slate-700 rounded-lg hover:bg-slate-50 transition-colors"
                >
                  Import Another File
                </button>
                <button
                  type="button"
                  onClick={() => navigate('/sdo/offence')}
                  className="px-4 py-2 text-sm font-semibold bg-blue-700 text-white rounded-lg hover:bg-blue-600 transition-colors"
                >
                  View OS Cases
                </button>
              </div>
            </div>
          )}

        </div>
      )}
    </div>
  );
}
