import { useState, useMemo, useCallback, memo, useDeferredValue } from 'react';
import { FileText, Download, RefreshCw, CheckSquare, Square, ArrowUp, ArrowDown, ArrowUpDown, Lock, Receipt, FileX } from 'lucide-react';
import api from '@/lib/api';
import DatePicker from '@/components/DatePicker';
import { showDownloadToast } from '@/components/DownloadToast';

// ── Column definitions ────────────────────────────────────────────────────────

interface ColDef { key: string; label: string }
interface ColGroup { group: string; cols: ColDef[] }

const BR_MASTER_GROUPS: ColGroup[] = [
  {
    group: 'BR Core',
    cols: [
      { key: 'br_no',      label: 'BR No.' },
      { key: 'br_year',    label: 'BR Year' },
      { key: 'br_date',    label: 'BR Date' },
      { key: 'br_type',    label: 'BR Type' },
      { key: 'br_shift',   label: 'BR Shift' },
      { key: 'actual_br_type', label: 'Actual BR Type' },
      { key: 'br_printed', label: 'BR Printed' },
    ],
  },
  {
    group: 'Passenger',
    cols: [
      { key: 'pax_name',             label: 'Passenger Name' },
      { key: 'pax_nationality',      label: 'Nationality' },
      { key: 'passport_no',          label: 'Passport No.' },
      { key: 'passport_date',        label: 'Passport Date' },
      { key: 'passport_issue_place', label: 'Passport Issue Place' },
      { key: 'pax_address1',         label: 'Address Line 1' },
      { key: 'pax_address2',         label: 'Address Line 2' },
      { key: 'pax_address3',         label: 'Address Line 3' },
      { key: 'pax_date_of_birth',    label: 'Date of Birth' },
      { key: 'pax_status',           label: 'Passenger Status' },
      { key: 'residence_at',         label: 'Residence' },
      { key: 'country_of_departure', label: 'Country of Departure' },
      { key: 'departure_date',       label: 'Departure Date' },
      { key: 'abroad_stay',          label: 'Stay Abroad (Days)' },
      { key: 'ff_ind',               label: 'Frequent Flier' },
      { key: 'arrived_from',         label: 'Arrived From' },
    ],
  },
  {
    group: 'Flight',
    cols: [
      { key: 'flight_no',   label: 'Flight No.' },
      { key: 'flight_date', label: 'Flight Date' },
    ],
  },
  {
    group: 'Linked Records',
    cols: [
      { key: 'os_no',   label: 'OS No.' },
      { key: 'os_date', label: 'OS Date' },
      { key: 'dr_no',   label: 'DR No.' },
      { key: 'dr_date', label: 'DR Date' },
    ],
  },
  {
    group: 'Financials',
    cols: [
      { key: 'total_items_value',  label: 'Total Items Value' },
      { key: 'total_fa_value',     label: 'Free Allowance Value' },
      { key: 'total_fa_availed',   label: 'FA Availed' },
      { key: 'total_duty_amount',  label: 'Total Duty' },
      { key: 'rf_amount',          label: 'RF Amount' },
      { key: 'pp_amount',          label: 'PP Amount' },
      { key: 'ref_amount',         label: 'Ref Amount' },
      { key: 'wh_amount',          label: 'WH Amount' },
      { key: 'other_amount',       label: 'Other Amount' },
      { key: 'br_amount',          label: 'BR Amount' },
      { key: 'total_payable',      label: 'Total Payable' },
      { key: 'br_amount_str',      label: 'BR Amount (Text)' },
      { key: 'br_no_str',          label: 'BR No. (Text)' },
    ],
  },
  {
    group: 'Bank & Batch',
    cols: [
      { key: 'challan_no',  label: 'Challan No.' },
      { key: 'bank_date',   label: 'Bank Date' },
      { key: 'bank_shift',  label: 'Bank Shift' },
      { key: 'batch_date',  label: 'Batch Date' },
      { key: 'batch_shift', label: 'Batch Shift' },
    ],
  },
  {
    group: 'Administrative',
    cols: [
      { key: 'dc_code',        label: 'DC Code' },
      { key: 'unique_no',      label: 'Unique No.' },
      { key: 'location_code',  label: 'Location Code' },
      { key: 'login_id',       label: 'Login ID' },
      { key: 'table_name',     label: 'Table Name' },
      { key: 'image_filename', label: 'Image File' },
      { key: 'bkup_taken',     label: 'Backup Taken' },
      { key: 'entry_deleted',  label: 'Entry Deleted' },
    ],
  },
];

const BR_ITEM_GROUP: ColGroup = {
  group: 'BR Items',
  cols: [
    { key: 'items_sno',              label: 'S.No.' },
    { key: 'items_desc',             label: 'Description' },
    { key: 'items_qty',              label: 'Quantity' },
    { key: 'items_uqc',              label: 'Unit' },
    { key: 'items_value',            label: 'Value' },
    { key: 'items_fa',               label: 'Free Allowance' },
    { key: 'items_bcd',              label: 'BCD' },
    { key: 'items_cvd',              label: 'CVD' },
    { key: 'items_cess',             label: 'Cess' },
    { key: 'items_hec',              label: 'HEC' },
    { key: 'items_duty',             label: 'Item Duty' },
    { key: 'items_duty_type',        label: 'Duty Type' },
    { key: 'items_category',         label: 'Category' },
    { key: 'items_release_category', label: 'Release Category' },
    { key: 'items_dr_no',            label: 'Item DR No.' },
    { key: 'items_dr_year',          label: 'Item DR Year' },
    { key: 'flight_no',              label: 'Item Flight No.' },
    { key: 'bank_date',              label: 'Item Bank Date' },
    { key: 'bank_shift',             label: 'Item Bank Shift' },
    { key: 'batch_date',             label: 'Item Batch Date' },
    { key: 'batch_shift',            label: 'Item Batch Shift' },
    { key: 'unique_no',              label: 'Item Unique No.' },
    { key: 'location_code',          label: 'Item Location' },
    { key: 'login_id',               label: 'Item Login ID' },
    { key: 'entry_deleted',          label: 'Item Deleted' },
  ],
};

const DR_MASTER_GROUPS: ColGroup[] = [
  {
    group: 'DR Core',
    cols: [
      { key: 'dr_no',      label: 'DR No.' },
      { key: 'dr_year',    label: 'DR Year' },
      { key: 'dr_date',    label: 'DR Date' },
      { key: 'dr_type',    label: 'DR Type' },
      { key: 'dr_printed', label: 'DR Printed' },
    ],
  },
  {
    group: 'Passenger',
    cols: [
      { key: 'pax_name',      label: 'Passenger Name' },
      { key: 'passport_no',   label: 'Passport No.' },
      { key: 'passport_date', label: 'Passport Date' },
      { key: 'pax_address1',  label: 'Address Line 1' },
      { key: 'pax_address2',  label: 'Address Line 2' },
      { key: 'pax_address3',  label: 'Address Line 3' },
    ],
  },
  {
    group: 'Travel',
    cols: [
      { key: 'port_of_departure', label: 'Port of Departure' },
      { key: 'flight_no',         label: 'Flight No.' },
      { key: 'flight_date',       label: 'Flight Date' },
    ],
  },
  {
    group: 'Detention',
    cols: [
      { key: 'detained_by',        label: 'Detained By' },
      { key: 'detained_pkg_no',    label: 'Package No.' },
      { key: 'detained_pkg_type',  label: 'Package Type' },
      { key: 'seal_no',            label: 'Seal No.' },
      { key: 'detention_reasons',  label: 'Detention Reasons' },
      { key: 'receipt_by_who',     label: 'Receipt By' },
      { key: 'seizure_date',       label: 'Seizure Date' },
    ],
  },
  {
    group: 'Linked Records',
    cols: [
      { key: 'os_no',        label: 'OS No.' },
      { key: 'warehouse_no', label: 'Warehouse No.' },
    ],
  },
  {
    group: 'Values',
    cols: [
      { key: 'total_items_value', label: 'Total Items Value' },
    ],
  },
  {
    group: 'Closure',
    cols: [
      { key: 'closure_ind',        label: 'Closure Status' },
      { key: 'closure_remarks',    label: 'Closure Remarks' },
      { key: 'closure_date',       label: 'Closure Date' },
      { key: 'closed_batch_date',  label: 'Closed Batch Date' },
      { key: 'closed_batch_shift', label: 'Closed Batch Shift' },
    ],
  },
  {
    group: 'Administrative',
    cols: [
      { key: 'unique_no',     label: 'Unique No.' },
      { key: 'location_code', label: 'Location Code' },
      { key: 'login_id',      label: 'Login ID' },
      { key: 'entry_deleted', label: 'Entry Deleted' },
    ],
  },
];

const DR_ITEM_GROUP: ColGroup = {
  group: 'DR Items',
  cols: [
    { key: 'items_sno',              label: 'S.No.' },
    { key: 'items_desc',             label: 'Description' },
    { key: 'items_qty',              label: 'Quantity' },
    { key: 'items_uqc',              label: 'Unit' },
    { key: 'items_value',            label: 'Value' },
    { key: 'items_fa',               label: 'Free Allowance' },
    { key: 'items_release_category', label: 'Release Category' },
    { key: 'receipt_by_who',         label: 'Receipt By' },
    { key: 'item_closure_remarks',   label: 'Item Closure Remarks' },
    { key: 'detained_pkg_no',        label: 'Package No.' },
    { key: 'detained_pkg_type',      label: 'Package Type' },
    { key: 'unique_no',              label: 'Item Unique No.' },
    { key: 'location_code',          label: 'Item Location' },
  ],
};

const BR_TYPES = ['Bagg', 'OS', 'OOS', 'SDO', 'Gold', 'Silv', 'Fuel', 'TR'];
const DR_TYPES = ['Bagg', 'AIU', 'MHB', 'Other'];

// These 3 are always auto-included when any item column is selected (context)
const ITEM_CONTEXT_KEYS = new Set(['items_sno', 'items_desc', 'items_qty']);

// ── Helpers ───────────────────────────────────────────────────────────────────

async function exportCsv(reportType: string, columns: string[], rows: Record<string, string>[], colLabels: Record<string, string>) {
  const header = columns.map(c => colLabels[c] ?? c).join(',');
  const body = rows.map(r =>
    columns.map(c => {
      const v = r[c] ?? '';
      return v.includes(',') || v.includes('"') || v.includes('\n')
        ? `"${v.replace(/"/g, '""')}"` : v;
    }).join(',')
  ).join('\n');
  const csvString = header + '\n' + body;
  const defaultName = `cops_${reportType.toLowerCase()}_report_${new Date().toISOString().slice(0, 10)}.csv`;

  try {
    const { save } = await import('@tauri-apps/plugin-dialog');
    const { writeTextFile } = await import('@tauri-apps/plugin-fs');
    const savePath = await save({ title: `Save ${reportType} Report CSV`, defaultPath: defaultName, filters: [{ name: 'CSV', extensions: ['csv'] }] });
    if (savePath) {
      await writeTextFile(savePath, csvString);
      showDownloadToast(`Report saved to ${savePath}`);
    }
  } catch {
    const blob = new Blob([csvString], { type: 'text/csv' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url; a.download = defaultName; a.click();
    URL.revokeObjectURL(url);
    showDownloadToast(`Report downloaded as ${defaultName}`);
  }
}

function ColSortIcon({ col, sortCol, sortDir }: { col: string; sortCol: string | null; sortDir: string }) {
  if (sortCol !== col) return <ArrowUpDown size={10} className="ml-1 opacity-30" />;
  return sortDir === 'asc'
    ? <ArrowUp size={10} className="ml-1 text-emerald-600" />
    : <ArrowDown size={10} className="ml-1 text-emerald-600" />;
}

const ItemCell = memo(function ItemCell({ value }: { value: string }) {
  const parts = value.split('\n');
  if (parts.length <= 1) return <span>{value || '—'}</span>;
  return (
    <div className="divide-y divide-amber-100 -my-1">
      {parts.map((p, i) => (
        <div key={i} className="py-0.5 text-xs">{p || '—'}</div>
      ))}
    </div>
  );
});

// Memoized row component — avoids re-rendering all rows on sort/filter state change.
const ResultRow = memo(function ResultRow({
  row, columns, itemColKeys, index,
}: {
  row: Record<string, string>;
  columns: string[];
  itemColKeys: Set<string>;
  index: number;
}) {
  return (
    <tr className={`border-b border-slate-50 ${index % 2 === 0 ? '' : 'bg-slate-50/50'} hover:bg-emerald-50/30`}>
      <td className="px-3 py-1.5 text-slate-400 tabular-nums align-top">{index + 1}</td>
      {columns.map(col => {
        const isItem = itemColKeys.has(col);
        return (
          <td key={col} className={`px-3 py-1.5 align-top ${isItem ? 'text-amber-800' : 'text-slate-700'} max-w-[220px]`}>
            {isItem
              ? <ItemCell value={row[col] ?? ''} />
              : <span className="truncate block max-w-[220px]">{row[col] ?? ''}</span>
            }
          </td>
        );
      })}
    </tr>
  );
});

// ── Component ─────────────────────────────────────────────────────────────────

type ReportType = 'BR' | 'DR';

export default function CustomReportBRDR() {
  const [reportType, setReportType] = useState<ReportType>('BR');

  // Separate selection state for BR and DR so switching modes doesn't lose selections
  const [selectedBrMaster, setSelectedBrMaster] = useState<Set<string>>(new Set(['br_no', 'br_year', 'br_date', 'pax_name']));
  const [selectedBrItems,  setSelectedBrItems]  = useState<Set<string>>(new Set());
  const [selectedDrMaster, setSelectedDrMaster] = useState<Set<string>>(new Set(['dr_no', 'dr_year', 'dr_date', 'pax_name']));
  const [selectedDrItems,  setSelectedDrItems]  = useState<Set<string>>(new Set());

  const selectedMaster = reportType === 'BR' ? selectedBrMaster : selectedDrMaster;
  const selectedItems  = reportType === 'BR' ? selectedBrItems  : selectedDrItems;
  const setSelectedMaster = reportType === 'BR' ? setSelectedBrMaster : setSelectedDrMaster;
  const setSelectedItems  = reportType === 'BR' ? setSelectedBrItems  : setSelectedDrItems;

  const masterGroups = reportType === 'BR' ? BR_MASTER_GROUPS : DR_MASTER_GROUPS;
  const itemGroup    = reportType === 'BR' ? BR_ITEM_GROUP    : DR_ITEM_GROUP;
  const typeOptions  = reportType === 'BR' ? BR_TYPES         : DR_TYPES;

  const itemColKeys = useMemo(() => new Set(itemGroup.cols.map(c => c.key)), [itemGroup]);

  // Date filters
  const [fromDate, setFromDate] = useState('');
  const [toDate,   setToDate]   = useState('');

  // Row-level filters
  const [filterDocNo,      setFilterDocNo]      = useState('');
  const [filterDocYear,    setFilterDocYear]    = useState('');
  const [filterDocType,    setFilterDocType]    = useState('');
  const [filterFlightNo,   setFilterFlightNo]   = useState('');
  const [filterPaxName,    setFilterPaxName]    = useState('');
  const [filterPassportNo, setFilterPassportNo] = useState('');
  const [filterOsNo,       setFilterOsNo]       = useState('');
  const [filterItemDesc,   setFilterItemDesc]   = useState('');

  const [loading, setLoading] = useState(false);
  const [result, setResult]   = useState<{ columns: string[]; rows: Record<string, string>[]; total: number } | null>(null);
  const [error, setError]     = useState('');
  const [sortCol, setSortCol] = useState<string | null>(null);
  const [sortDir, setSortDir] = useState<'asc' | 'desc'>('asc');

  const effectiveItemCols: string[] = useMemo(() => {
    if (selectedItems.size === 0) return [];
    return [...new Set([...ITEM_CONTEXT_KEYS, ...selectedItems])];
  }, [selectedItems]);

  // Defer sort state so typing/clicking stays instant — React renders table in lower priority.
  const deferredSortCol = useDeferredValue(sortCol);
  const deferredSortDir = useDeferredValue(sortDir);

  const sortedRows = useMemo(() => {
    if (!result) return [];
    if (!deferredSortCol) return result.rows.slice(0, 500);

    // Detect column type once from first non-empty value — avoids per-comparison parseFloat.
    const col = deferredSortCol;
    let isNumeric = false;
    for (let i = 0; i < result.rows.length && i < 20; i++) {
      const v = result.rows[i][col];
      if (v != null && v !== '') { isNumeric = !isNaN(parseFloat(v)) && isFinite(Number(v)); break; }
    }
    const dir = deferredSortDir === 'asc' ? 1 : -1;
    const collator = new Intl.Collator(undefined, { sensitivity: 'base', numeric: true });

    const copy = result.rows.slice();
    if (isNumeric) {
      copy.sort((a, b) => ((parseFloat(a[col]) || 0) - (parseFloat(b[col]) || 0)) * dir);
    } else {
      copy.sort((a, b) => collator.compare(a[col] ?? '', b[col] ?? '') * dir);
    }
    return copy.slice(0, 500);
  }, [result, deferredSortCol, deferredSortDir]);

  // For CSV export (uses non-deferred current sort state) — only built when download is clicked.
  const sortComparator = useCallback((a: Record<string, string>, b: Record<string, string>) => {
    if (!sortCol) return 0;
    const av = a[sortCol] ?? '';
    const bv = b[sortCol] ?? '';
    const an = parseFloat(av);
    const bn = parseFloat(bv);
    const cmp = !isNaN(an) && !isNaN(bn) ? an - bn : av.localeCompare(bv, undefined, { sensitivity: 'base' });
    return sortDir === 'asc' ? cmp : -cmp;
  }, [sortCol, sortDir]);

  const handleColSort = (col: string) => {
    if (sortCol === col) { setSortDir(d => d === 'asc' ? 'desc' : 'asc'); }
    else { setSortCol(col); setSortDir('asc'); }
  };

  const labelOf = useMemo(() => {
    const map: Record<string, string> = {};
    masterGroups.forEach(g => g.cols.forEach(c => { map[c.key] = c.label; }));
    itemGroup.cols.forEach(c => { map[c.key] = c.label; });
    return map;
  }, [masterGroups, itemGroup]);

  const toggle = (key: string, which: 'master' | 'item') => {
    const set = which === 'master' ? new Set(selectedMaster) : new Set(selectedItems);
    const setter = which === 'master' ? setSelectedMaster : setSelectedItems;
    if (which === 'item' && ITEM_CONTEXT_KEYS.has(key) && set.size > 1) return;
    set.has(key) ? set.delete(key) : set.add(key);
    setter(set);
    setResult(null);
  };

  const toggleGroup = (cols: ColDef[], which: 'master' | 'item') => {
    const set = which === 'master' ? new Set(selectedMaster) : new Set(selectedItems);
    const setter = which === 'master' ? setSelectedMaster : setSelectedItems;
    const allSelected = cols.every(c => set.has(c.key));
    cols.forEach(c => allSelected ? set.delete(c.key) : set.add(c.key));
    setter(set);
    setResult(null);
  };

  const switchReportType = (t: ReportType) => {
    if (t === reportType) return;
    setReportType(t);
    setResult(null);
    setError('');
    setSortCol(null);
    setSortDir('asc');
    // Clear all row filters — BR and DR have different semantics (e.g. doc_no maps to
    // br_no vs dr_no), so carrying filters silently across modes is a footgun.
    setFilterDocNo('');
    setFilterDocYear('');
    setFilterDocType('');
    setFilterFlightNo('');
    setFilterPaxName('');
    setFilterPassportNo('');
    setFilterOsNo('');
    setFilterItemDesc('');
  };

  const hasActiveFilters = filterDocNo || filterDocYear || filterDocType || filterFlightNo || filterPaxName || filterPassportNo || filterOsNo || filterItemDesc;

  const generate = async () => {
    if (selectedMaster.size === 0 && selectedItems.size === 0) {
      setError('Select at least one column.'); return;
    }
    setError(''); setLoading(true); setResult(null); setSortCol(null); setSortDir('asc');
    try {
      const res = await api.post('/backup/custom-report-brdr', {
        report_type: reportType,
        master_cols: [...selectedMaster],
        item_cols:   effectiveItemCols,
        from_date:   fromDate || null,
        to_date:     toDate   || null,
        doc_no:      filterDocNo      ? parseInt(filterDocNo) : null,
        doc_year:    filterDocYear    ? parseInt(filterDocYear) : null,
        doc_type:    filterDocType.trim()    || null,
        flight_no:   filterFlightNo.trim()   || null,
        pax_name:    filterPaxName.trim()    || null,
        passport_no: filterPassportNo.trim() || null,
        os_no:       filterOsNo.trim()       || null,
        item_desc:   filterItemDesc.trim()   || null,
      });
      setResult(res.data);
    } catch (err: any) {
      let detail = err.response?.data?.detail || 'Failed to generate report.';
      if (Array.isArray(detail)) detail = detail.map((e: any) => `${e.loc?.join('.')} - ${e.msg}`).join(', ');
      else if (typeof detail === 'object') detail = JSON.stringify(detail);
      setError(detail);
    } finally {
      setLoading(false);
    }
  };

  const totalSelected = selectedMaster.size + selectedItems.size;
  const inp = "w-full border border-slate-300 rounded-md px-2 py-1.5 text-xs focus:ring-1 focus:ring-emerald-400 focus:border-emerald-400 bg-white";
  const docNoLabel   = reportType === 'BR' ? 'BR No.'   : 'DR No.';
  const docYearLabel = reportType === 'BR' ? 'BR Year'  : 'DR Year';
  const docTypeLabel = reportType === 'BR' ? 'BR Type'  : 'DR Type';

  return (
    <div className="space-y-5">
      {/* Header */}
      <div>
        <h1 className="text-lg font-bold text-slate-800 flex items-center gap-2">
          <FileText size={20} className="text-emerald-600" />
          Custom Report — BR / DR
        </h1>
        <p className="text-xs text-slate-500 mt-1">
          Choose any columns from baggage or detention registers, apply filters, then generate.
        </p>
      </div>

      {/* Report type toggle */}
      <div className="inline-flex rounded-lg border border-slate-300 bg-white overflow-hidden">
        <button
          onClick={() => switchReportType('BR')}
          className={`flex items-center gap-2 px-4 py-2 text-xs font-medium transition-colors ${reportType === 'BR' ? 'bg-emerald-600 text-white' : 'text-slate-600 hover:bg-slate-50'}`}
        >
          <Receipt size={14} />
          Baggage Receipts
        </button>
        <button
          onClick={() => switchReportType('DR')}
          className={`flex items-center gap-2 px-4 py-2 text-xs font-medium border-l border-slate-300 transition-colors ${reportType === 'DR' ? 'bg-emerald-600 text-white' : 'text-slate-600 hover:bg-slate-50'}`}
        >
          <FileX size={14} />
          Detention Receipts
        </button>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">

        {/* ── Column Selector ── */}
        <div className="lg:col-span-1 space-y-3">
          {masterGroups.map(g => {
            const allSel = g.cols.every(c => selectedMaster.has(c.key));
            return (
              <div key={g.group} className="bg-white rounded-xl border border-slate-200 overflow-hidden">
                <div className="flex items-center justify-between px-3 py-2 bg-slate-50 border-b border-slate-100">
                  <span className="text-xs font-semibold text-slate-600 uppercase tracking-wide">{g.group}</span>
                  <button onClick={() => toggleGroup(g.cols, 'master')}
                    className="text-[10px] text-emerald-600 hover:text-emerald-800 font-medium">
                    {allSel ? 'Deselect all' : 'Select all'}
                  </button>
                </div>
                <div className="p-2 space-y-0.5">
                  {g.cols.map(c => (
                    <button key={c.key} type="button"
                      onClick={() => toggle(c.key, 'master')}
                      className="flex items-center gap-2 px-1 py-0.5 rounded hover:bg-slate-50 cursor-pointer w-full text-left focus:outline-none">
                      <span className="text-emerald-600 shrink-0">
                        {selectedMaster.has(c.key) ? <CheckSquare size={13} /> : <Square size={13} className="text-slate-300" />}
                      </span>
                      <span className="text-xs text-slate-700">{c.label}</span>
                    </button>
                  ))}
                </div>
              </div>
            );
          })}

          {/* Items group */}
          {(() => {
            const allSel = itemGroup.cols.every(c => selectedItems.has(c.key) || (ITEM_CONTEXT_KEYS.has(c.key) && selectedItems.size > 0));
            const hasAnyItem = selectedItems.size > 0;
            return (
              <div className="bg-white rounded-xl border border-amber-200 overflow-hidden">
                <div className="flex items-center justify-between px-3 py-2 bg-amber-50 border-b border-amber-100">
                  <span className="text-xs font-semibold text-amber-700 uppercase tracking-wide">
                    {itemGroup.group}
                  </span>
                  <button onClick={() => toggleGroup(itemGroup.cols, 'item')}
                    className="text-[10px] text-amber-600 hover:text-amber-800 font-medium">
                    {allSel ? 'Deselect all' : 'Select all'}
                  </button>
                </div>
                {hasAnyItem && (
                  <div className="px-3 py-1.5 bg-amber-50/60 border-b border-amber-100 flex items-center gap-1.5">
                    <Lock size={10} className="text-amber-500" />
                    <span className="text-[10px] text-amber-600">S.No., Desc &amp; Qty always included</span>
                  </div>
                )}
                <div className="p-2 space-y-0.5">
                  {itemGroup.cols.map(c => {
                    const isContext = ITEM_CONTEXT_KEYS.has(c.key);
                    const isForced  = isContext && hasAnyItem;
                    const isChecked = selectedItems.has(c.key) || isForced;
                    return (
                      <button key={c.key} type="button"
                        onClick={() => !isForced && toggle(c.key, 'item')}
                        className={`flex items-center gap-2 px-1 py-0.5 rounded w-full text-left focus:outline-none ${isForced ? 'cursor-default opacity-70' : 'hover:bg-amber-50 cursor-pointer'}`}>
                        <span className={isForced ? 'text-amber-400 shrink-0' : 'text-amber-500 shrink-0'}>
                          {isChecked ? <CheckSquare size={13} /> : <Square size={13} className="text-slate-300" />}
                        </span>
                        <span className="text-xs text-slate-700">{c.label}</span>
                        {isForced && <Lock size={9} className="text-amber-400 ml-auto shrink-0" />}
                      </button>
                    );
                  })}
                </div>
              </div>
            );
          })()}
        </div>

        {/* ── Controls + Results ── */}
        <div className="lg:col-span-2 space-y-4">

          <div className="bg-white rounded-xl border border-slate-200 p-4 space-y-3">
            <p className="text-xs font-semibold text-slate-600">
              {totalSelected} column{totalSelected !== 1 ? 's' : ''} selected
              {selectedItems.size > 0 && (
                <span className="ml-2 text-amber-600">(items stacked per row)</span>
              )}
            </p>

            {/* Date + Type */}
            <div className="grid grid-cols-3 gap-3">
              <div>
                <label className="block text-xs text-slate-500 mb-1">From Date</label>
                <DatePicker value={fromDate} onChange={setFromDate} inputClassName="input-field" />
              </div>
              <div>
                <label className="block text-xs text-slate-500 mb-1">To Date</label>
                <DatePicker value={toDate} onChange={setToDate} inputClassName="input-field" />
              </div>
              <div>
                <label className="block text-xs text-slate-500 mb-1">{docTypeLabel}</label>
                <select value={filterDocType} onChange={e => setFilterDocType(e.target.value)}
                  className="w-full bg-white border border-slate-300 rounded-md text-xs px-3 py-2 text-slate-800 focus:ring-emerald-500 focus:border-emerald-500">
                  <option value="">All Types</option>
                  {typeOptions.map(t => <option key={t} value={t}>{t}</option>)}
                </select>
              </div>
            </div>

            {/* Row filters */}
            <div className="border border-slate-100 rounded-lg p-3 bg-slate-50/60 space-y-2">
              <p className="text-[10px] font-semibold text-slate-500 uppercase tracking-wide flex items-center gap-1">
                Row Filters <span className={`ml-1 px-1.5 py-0.5 rounded text-[9px] font-bold ${hasActiveFilters ? 'bg-emerald-100 text-emerald-700' : 'bg-slate-200 text-slate-500'}`}>{hasActiveFilters ? 'active' : 'optional'}</span>
              </p>
              <div className="grid grid-cols-2 gap-2">
                <div>
                  <label className="block text-[10px] text-slate-500 mb-0.5">{docNoLabel}</label>
                  <input type="number" value={filterDocNo} onChange={e => setFilterDocNo(e.target.value)} className={inp} placeholder="e.g. 142" />
                </div>
                <div>
                  <label className="block text-[10px] text-slate-500 mb-0.5">{docYearLabel}</label>
                  <input type="number" value={filterDocYear} onChange={e => setFilterDocYear(e.target.value)} className={inp} placeholder="e.g. 2024" />
                </div>
                <div>
                  <label className="block text-[10px] text-slate-500 mb-0.5">Passenger Name</label>
                  <input type="text" value={filterPaxName} onChange={e => setFilterPaxName(e.target.value)} className={inp} placeholder="Partial match" />
                </div>
                <div>
                  <label className="block text-[10px] text-slate-500 mb-0.5">Passport No.</label>
                  <input type="text" value={filterPassportNo} onChange={e => setFilterPassportNo(e.target.value)} className={inp} placeholder="Partial match" />
                </div>
                <div>
                  <label className="block text-[10px] text-slate-500 mb-0.5">Flight No.</label>
                  <input type="text" value={filterFlightNo} onChange={e => setFilterFlightNo(e.target.value)} className={inp} placeholder="e.g. EK542" />
                </div>
                <div>
                  <label className="block text-[10px] text-slate-500 mb-0.5">Linked OS No.</label>
                  <input type="text" value={filterOsNo} onChange={e => setFilterOsNo(e.target.value)} className={inp} placeholder="Partial match" />
                </div>
                <div className="col-span-2">
                  <label className="block text-[10px] text-slate-500 mb-0.5">Item Description</label>
                  <input type="text" value={filterItemDesc} onChange={e => setFilterItemDesc(e.target.value)} className={inp} placeholder="e.g. Gold, Drone… (partial match)" />
                </div>
              </div>
              {hasActiveFilters && (
                <button onClick={() => { setFilterDocNo(''); setFilterDocYear(''); setFilterDocType(''); setFilterFlightNo(''); setFilterPaxName(''); setFilterPassportNo(''); setFilterOsNo(''); setFilterItemDesc(''); setResult(null); }}
                  className="text-[10px] text-slate-500 hover:text-red-600 underline">
                  Clear all row filters
                </button>
              )}
            </div>

            <div className="flex gap-2 flex-wrap">
              <button onClick={generate} disabled={loading || totalSelected === 0}
                className="flex items-center gap-2 px-4 py-2 text-xs rounded-lg bg-emerald-600 text-white hover:bg-emerald-700 disabled:opacity-50">
                <RefreshCw size={12} className={loading ? 'animate-spin' : ''} />
                {loading ? 'Generating…' : 'Generate Report'}
              </button>
              {result && result.rows.length > 0 && (
                <button onClick={() => exportCsv(reportType, result.columns, sortCol ? [...result.rows].sort(sortComparator) : result.rows, labelOf)}
                  className="flex items-center gap-2 px-4 py-2 text-xs rounded-lg bg-slate-700 text-white hover:bg-slate-800">
                  <Download size={12} />
                  Download CSV ({result.total.toLocaleString()} rows)
                </button>
              )}
            </div>
            {error && <p className="text-xs text-red-600">{error}</p>}
          </div>

          {/* Results */}
          {result && (
            <div className="bg-white rounded-xl border border-slate-200 overflow-hidden">
              <div className="px-4 py-2.5 border-b border-slate-100 flex items-center justify-between">
                <span className="text-xs font-semibold text-slate-600">
                  Results — {result.total.toLocaleString()} {reportType} record{result.total !== 1 ? 's' : ''}
                </span>
                {result.total > 500 && (
                  <span className="text-[10px] text-amber-600 bg-amber-50 px-2 py-0.5 rounded">
                    Showing first 500 — download CSV for full data
                  </span>
                )}
              </div>
              <div className="overflow-auto max-h-[60vh]">
                <table className="w-full text-xs border-collapse">
                  <thead className="sticky top-0 bg-slate-50 z-10">
                    <tr>
                      <th className="text-left px-3 py-2 text-slate-500 font-medium border-b border-slate-100 whitespace-nowrap">#</th>
                      {result.columns.map(col => (
                        <th key={col}
                          onClick={() => handleColSort(col)}
                          className={`text-left px-3 py-2 font-medium border-b border-slate-100 whitespace-nowrap cursor-pointer select-none hover:bg-slate-100 transition-colors ${
                            itemColKeys.has(col)
                              ? 'text-amber-700 bg-amber-50 hover:bg-amber-100'
                              : 'text-slate-600'
                          }`}>
                          <span className="flex items-center">
                            {labelOf[col] ?? col}
                            <ColSortIcon col={col} sortCol={sortCol} sortDir={sortDir} />
                          </span>
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {sortedRows.map((row, i) => (
                      <ResultRow key={i} row={row} columns={result.columns} itemColKeys={itemColKeys} index={i} />
                    ))}
                  </tbody>
                </table>
                {result.rows.length === 0 && (
                  <p className="text-xs text-slate-400 text-center py-8">No records found for the selected filters.</p>
                )}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
