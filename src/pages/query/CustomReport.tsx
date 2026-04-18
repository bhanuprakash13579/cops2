import { useState, useMemo, useCallback } from 'react';
import { FileText, Download, RefreshCw, CheckSquare, Square, ArrowUp, ArrowDown, ArrowUpDown, Lock } from 'lucide-react';
import api from '@/lib/api';
import DatePicker from '@/components/DatePicker';
import { showDownloadToast } from '@/components/DownloadToast';

// ── Column definitions ────────────────────────────────────────────────────────

interface ColDef { key: string; label: string }

const MASTER_GROUPS: { group: string; cols: ColDef[] }[] = [
  {
    group: 'OS Case',
    cols: [
      { key: 'os_no',           label: 'OS No.' },
      { key: 'os_year',         label: 'Year' },
      { key: 'os_date',         label: 'OS Date' },
      { key: 'location_code',   label: 'Location Code' },
      { key: 'case_type',       label: 'Case Type' },
      { key: 'os_category',     label: 'OS Category' },
      { key: 'booked_by',       label: 'Booked By' },
    ],
  },
  {
    group: 'Passenger',
    cols: [
      { key: 'pax_name',              label: 'Passenger Name' },
      { key: 'pax_nationality',       label: 'Nationality' },
      { key: 'passport_no',           label: 'Passport No.' },
      { key: 'passport_date',         label: 'Passport Date' },
      { key: 'pp_issue_place',        label: 'Passport Issue Place' },
      { key: 'pax_date_of_birth',     label: 'Date of Birth' },
      { key: 'father_name',           label: "Father's Name" },
      { key: 'pax_address1',          label: 'Address Line 1' },
      { key: 'pax_address2',          label: 'Address Line 2' },
      { key: 'pax_address3',          label: 'Address Line 3' },
      { key: 'residence_at',          label: 'Residence' },
      { key: 'country_of_departure',  label: 'Country of Departure' },
      { key: 'date_of_departure',     label: 'Date of Departure' },
      { key: 'port_of_dep_dest',      label: 'Port of Departure' },
      { key: 'stay_abroad_days',      label: 'Stay Abroad (Days)' },
      { key: 'old_passport_no',       label: 'Old Passport No.' },
      { key: 'pax_status',            label: 'Passenger Status' },
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
    group: 'Financials',
    cols: [
      { key: 'total_items',        label: 'Total Items' },
      { key: 'total_items_value',  label: 'Total Items Value' },
      { key: 'total_fa_value',     label: 'Free Allowance Value' },
      { key: 'dutiable_value',     label: 'Dutiable Value' },
      { key: 'total_duty_amount',  label: 'Total Duty' },
      { key: 'total_payable',      label: 'Total Payable' },
      { key: 'rf_amount',          label: 'RF Amount' },
      { key: 'pp_amount',          label: 'PP Amount' },
      { key: 'ref_amount',         label: 'Ref Amount' },
      { key: 'br_amount',          label: 'BR Amount' },
      { key: 'wh_amount',          label: 'WH Amount' },
      { key: 'other_amount',       label: 'Other Amount' },
      { key: 'redeemed_value',     label: 'Redeemed Value' },
      { key: 'confiscated_value',  label: 'Confiscated Value' },
      { key: 're_export_value',    label: 'Re-export Value' },
    ],
  },
  {
    group: 'Adjudication',
    cols: [
      { key: 'adjudication_date',      label: 'Adjudication Date' },
      { key: 'adj_offr_name',          label: 'Adjudicating Officer' },
      { key: 'adj_offr_designation',   label: 'Officer Designation' },
      { key: 'adjn_offr_remarks',      label: 'Officer Remarks' },
      { key: 'online_adjn',            label: 'Online Adjudication' },
    ],
  },
  {
    group: 'BR / DR Linkage',
    cols: [
      { key: 'br_no_num',     label: 'BR No.' },
      { key: 'br_date_str',   label: 'BR Date' },
      { key: 'br_no_str',     label: 'BR No. (Text)' },
      { key: 'br_amount_str', label: 'BR Amount (Text)' },
      { key: 'dr_no',         label: 'DR No.' },
      { key: 'dr_year',       label: 'DR Year' },
    ],
  },
  {
    group: 'Post-Adjudication',
    cols: [
      { key: 'post_adj_br_entries', label: 'BR No(s) with Dates' },
      { key: 'post_adj_dr_no',      label: 'Post-Adj DR No.' },
      { key: 'post_adj_dr_date',    label: 'Post-Adj DR Date' },
    ],
  },
  {
    group: 'Other',
    cols: [
      { key: 'seizure_date',   label: 'Seizure Date' },
      { key: 'supdts_remarks', label: "Supdt's Remarks" },
    ],
  },
];

const ITEM_GROUP: { group: string; cols: ColDef[] } = {
  group: 'Items (cops_items)',
  cols: [
    { key: 'items_desc',             label: 'Item Description' },
    { key: 'items_qty',              label: 'Quantity' },
    { key: 'items_uqc',             label: 'Unit' },
    { key: 'items_value',            label: 'Item Value' },
    { key: 'items_fa',               label: 'Free Allowance' },
    { key: 'items_duty',             label: 'Item Duty' },
    { key: 'items_duty_type',        label: 'Duty Type' },
    { key: 'items_category',         label: 'Category' },
    { key: 'items_sub_category',     label: 'Sub Category' },
    { key: 'items_release_category', label: 'Release Category' },
    { key: 'value_per_piece',        label: 'Value per Piece' },
    { key: 'cumulative_duty_rate',   label: 'Cumulative Duty Rate' },
  ],
};

// These 3 are always auto-included when any item column is selected.
// They form the minimum context needed to make item data readable.
const ITEM_CONTEXT_KEYS = new Set(['items_desc', 'items_qty', 'items_uqc']);

const ITEM_COL_KEYS = new Set(ITEM_GROUP.cols.map(c => c.key));

// ── Helpers ───────────────────────────────────────────────────────────────────

async function exportCsv(columns: string[], rows: Record<string, string>[], colLabels: Record<string, string>) {
  const header = columns.map(c => colLabels[c] ?? c).join(',');
  const body = rows.map(r =>
    columns.map(c => {
      const v = r[c] ?? '';
      return v.includes(',') || v.includes('"') || v.includes('\n')
        ? `"${v.replace(/"/g, '""')}"` : v;
    }).join(',')
  ).join('\n');
  const csvString = header + '\n' + body;
  const defaultName = `cops_report_${new Date().toISOString().slice(0, 10)}.csv`;

  try {
    const { save } = await import('@tauri-apps/plugin-dialog');
    const { writeTextFile } = await import('@tauri-apps/plugin-fs');
    const savePath = await save({ title: 'Save Report CSV', defaultPath: defaultName, filters: [{ name: 'CSV', extensions: ['csv'] }] });
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

/** Render a cell value. Item cells with \n are split into stacked sub-rows. */
function ItemCell({ value }: { value: string }) {
  const parts = value.split('\n');
  if (parts.length <= 1) return <span>{value || '—'}</span>;
  return (
    <div className="divide-y divide-amber-100 -my-1">
      {parts.map((p, i) => (
        <div key={i} className="py-0.5 text-xs">{p || '—'}</div>
      ))}
    </div>
  );
}

// ── Component ─────────────────────────────────────────────────────────────────

export default function CustomReport() {
  const [selectedMaster, setSelectedMaster] = useState<Set<string>>(new Set(['os_no', 'os_year', 'pax_name']));
  const [selectedItems, setSelectedItems] = useState<Set<string>>(new Set());

  // Date / case-type filters
  const [fromDate, setFromDate] = useState('');
  const [toDate, setToDate]     = useState('');
  const [caseType, setCaseType] = useState('');

  // Row-level filters
  const [filterOsNo,       setFilterOsNo]       = useState('');
  const [filterOsYear,     setFilterOsYear]     = useState('');
  const [filterAdjOfficer, setFilterAdjOfficer] = useState('');
  const [filterFlightNo,   setFilterFlightNo]   = useState('');
  const [filterPaxName,    setFilterPaxName]    = useState('');
  const [filterPassportNo, setFilterPassportNo] = useState('');
  const [filterItemDesc,   setFilterItemDesc]   = useState('');

  const [loading, setLoading]   = useState(false);
  const [result, setResult]     = useState<{ columns: string[]; rows: Record<string, string>[]; total: number } | null>(null);
  const [error, setError]       = useState('');
  const [sortCol, setSortCol]   = useState<string | null>(null);
  const [sortDir, setSortDir]   = useState<'asc' | 'desc'>('asc');

  // When any item column is selected, context columns are forced in.
  const effectiveItemCols: string[] = useMemo(() => {
    if (selectedItems.size === 0) return [];
    return [...new Set([...ITEM_CONTEXT_KEYS, ...selectedItems])];
  }, [selectedItems]);

  const sortComparator = useCallback((a: Record<string, string>, b: Record<string, string>) => {
    if (!sortCol) return 0;
    const av = a[sortCol] ?? '';
    const bv = b[sortCol] ?? '';
    const an = parseFloat(av);
    const bn = parseFloat(bv);
    const cmp = !isNaN(an) && !isNaN(bn) ? an - bn : av.localeCompare(bv, undefined, { sensitivity: 'base' });
    return sortDir === 'asc' ? cmp : -cmp;
  }, [sortCol, sortDir]);

  const sortedRows = useMemo(() => {
    if (!result) return [];
    if (!sortCol) return result.rows.slice(0, 500);
    return [...result.rows].sort(sortComparator).slice(0, 500);
  }, [result, sortCol, sortDir, sortComparator]);

  const handleColSort = (col: string) => {
    if (sortCol === col) { setSortDir(d => d === 'asc' ? 'desc' : 'asc'); }
    else { setSortCol(col); setSortDir('asc'); }
  };

  const labelOf: Record<string, string> = {};
  MASTER_GROUPS.forEach(g => g.cols.forEach(c => { labelOf[c.key] = c.label; }));
  ITEM_GROUP.cols.forEach(c => { labelOf[c.key] = c.label; });

  const toggle = (key: string, which: 'master' | 'item') => {
    const set    = which === 'master' ? new Set(selectedMaster) : new Set(selectedItems);
    const setter = which === 'master' ? setSelectedMaster       : setSelectedItems;
    // Context cols can't be unchecked individually while other item cols are selected
    if (which === 'item' && ITEM_CONTEXT_KEYS.has(key) && set.size > 1) return;
    set.has(key) ? set.delete(key) : set.add(key);
    setter(set);
    setResult(null);
  };

  const toggleGroup = (cols: ColDef[], which: 'master' | 'item') => {
    const set    = which === 'master' ? new Set(selectedMaster) : new Set(selectedItems);
    const setter = which === 'master' ? setSelectedMaster       : setSelectedItems;
    const allSelected = cols.every(c => set.has(c.key));
    cols.forEach(c => allSelected ? set.delete(c.key) : set.add(c.key));
    setter(set);
    setResult(null);
  };

  const hasActiveFilters = filterOsNo || filterOsYear || filterAdjOfficer || filterFlightNo || filterPaxName || filterPassportNo || filterItemDesc;

  const generate = async () => {
    if (selectedMaster.size === 0 && selectedItems.size === 0) {
      setError('Select at least one column.'); return;
    }
    setError(''); setLoading(true); setResult(null); setSortCol(null); setSortDir('asc');
    try {
      const res = await api.post('/backup/custom-report', {
        master_cols: [...selectedMaster],
        item_cols:   effectiveItemCols,
        from_date:   fromDate || null,
        to_date:     toDate   || null,
        case_type:   caseType || null,
        os_no:         filterOsNo       || null,
        os_year:       filterOsYear     ? parseInt(filterOsYear) : null,
        adj_offr_name: filterAdjOfficer || null,
        flight_no:     filterFlightNo   || null,
        pax_name:      filterPaxName    || null,
        passport_no:   filterPassportNo || null,
        item_desc:     filterItemDesc   || null,
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

  return (
    <div className="space-y-5">
      {/* Header */}
      <div>
        <h1 className="text-lg font-bold text-slate-800 flex items-center gap-2">
          <FileText size={20} className="text-emerald-600" />
          Custom Report
        </h1>
        <p className="text-xs text-slate-500 mt-1">
          Choose columns, apply filters, then generate. Item columns show all items stacked within each row.
        </p>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">

        {/* ── Column Selector ── */}
        <div className="lg:col-span-1 space-y-3">

          {MASTER_GROUPS.map(g => {
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
            const allSel = ITEM_GROUP.cols.every(c => selectedItems.has(c.key) || ITEM_CONTEXT_KEYS.has(c.key) && selectedItems.size > 0);
            const hasAnyItem = selectedItems.size > 0;
            return (
              <div className="bg-white rounded-xl border border-amber-200 overflow-hidden">
                <div className="flex items-center justify-between px-3 py-2 bg-amber-50 border-b border-amber-100">
                  <span className="text-xs font-semibold text-amber-700 uppercase tracking-wide">
                    {ITEM_GROUP.group}
                  </span>
                  <button onClick={() => toggleGroup(ITEM_GROUP.cols, 'item')}
                    className="text-[10px] text-amber-600 hover:text-amber-800 font-medium">
                    {allSel ? 'Deselect all' : 'Select all'}
                  </button>
                </div>
                {hasAnyItem && (
                  <div className="px-3 py-1.5 bg-amber-50/60 border-b border-amber-100 flex items-center gap-1.5">
                    <Lock size={10} className="text-amber-500" />
                    <span className="text-[10px] text-amber-600">Desc, Qty &amp; Unit always included</span>
                  </div>
                )}
                <div className="p-2 space-y-0.5">
                  {ITEM_GROUP.cols.map(c => {
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

          {/* Controls */}
          <div className="bg-white rounded-xl border border-slate-200 p-4 space-y-3">
            <p className="text-xs font-semibold text-slate-600">
              {totalSelected} column{totalSelected !== 1 ? 's' : ''} selected
              {selectedItems.size > 0 && (
                <span className="ml-2 text-amber-600">(items stacked per row)</span>
              )}
            </p>

            {/* Date + Case Type */}
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
                <label className="block text-xs text-slate-500 mb-1">Case Type</label>
                <select value={caseType} onChange={e => setCaseType(e.target.value)}
                  className="w-full bg-white border border-slate-300 rounded-md text-xs px-3 py-2 text-slate-800 focus:ring-emerald-500 focus:border-emerald-500">
                  <option value="">All Cases</option>
                  <option value="Arrival Case">Arrival Cases</option>
                  <option value="Export Case">Export Cases</option>
                </select>
              </div>
            </div>

            {/* Row-level filters */}
            <div className="border border-slate-100 rounded-lg p-3 bg-slate-50/60 space-y-2">
              <p className="text-[10px] font-semibold text-slate-500 uppercase tracking-wide flex items-center gap-1">
                Row Filters <span className={`ml-1 px-1.5 py-0.5 rounded text-[9px] font-bold ${hasActiveFilters ? 'bg-emerald-100 text-emerald-700' : 'bg-slate-200 text-slate-500'}`}>{hasActiveFilters ? 'active' : 'optional'}</span>
              </p>
              <div className="grid grid-cols-2 gap-2">
                <div>
                  <label className="block text-[10px] text-slate-500 mb-0.5">OS No.</label>
                  <input type="text" value={filterOsNo} onChange={e => setFilterOsNo(e.target.value)} className={inp} placeholder="e.g. 142" />
                </div>
                <div>
                  <label className="block text-[10px] text-slate-500 mb-0.5">OS Year</label>
                  <input type="number" value={filterOsYear} onChange={e => setFilterOsYear(e.target.value)} className={inp} placeholder="e.g. 2024" />
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
                  <label className="block text-[10px] text-slate-500 mb-0.5">Adjudicating Officer</label>
                  <input type="text" value={filterAdjOfficer} onChange={e => setFilterAdjOfficer(e.target.value)} className={inp} placeholder="Partial match" />
                </div>
                <div className="col-span-2">
                  <label className="block text-[10px] text-slate-500 mb-0.5">Item Description</label>
                  <input type="text" value={filterItemDesc} onChange={e => setFilterItemDesc(e.target.value)} className={inp} placeholder="e.g. Gold, Drone… (partial match)" />
                </div>
              </div>
              {hasActiveFilters && (
                <button onClick={() => { setFilterOsNo(''); setFilterOsYear(''); setFilterAdjOfficer(''); setFilterFlightNo(''); setFilterPaxName(''); setFilterPassportNo(''); setFilterItemDesc(''); setResult(null); }}
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
                <button onClick={() => exportCsv(result.columns, sortCol ? [...result.rows].sort(sortComparator) : result.rows, labelOf)}
                  className="flex items-center gap-2 px-4 py-2 text-xs rounded-lg bg-slate-700 text-white hover:bg-slate-800">
                  <Download size={12} />
                  Download CSV ({result.total.toLocaleString()} rows)
                </button>
              )}
            </div>
            {error && <p className="text-xs text-red-600">{error}</p>}
          </div>

          {/* Results table */}
          {result && (
            <div className="bg-white rounded-xl border border-slate-200 overflow-hidden">
              <div className="px-4 py-2.5 border-b border-slate-100 flex items-center justify-between">
                <span className="text-xs font-semibold text-slate-600">
                  Results — {result.total.toLocaleString()} OS case{result.total !== 1 ? 's' : ''}
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
                            ITEM_COL_KEYS.has(col)
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
                      <tr key={i} className={`border-b border-slate-50 ${i % 2 === 0 ? '' : 'bg-slate-50/50'} hover:bg-emerald-50/30`}>
                        <td className="px-3 py-1.5 text-slate-400 tabular-nums align-top">{i + 1}</td>
                        {result.columns.map(col => (
                          <td key={col} className={`px-3 py-1.5 align-top ${ITEM_COL_KEYS.has(col) ? 'text-amber-800' : 'text-slate-700'} max-w-[220px]`}>
                            {ITEM_COL_KEYS.has(col)
                              ? <ItemCell value={row[col] ?? ''} />
                              : <span className="truncate block max-w-[220px]">{row[col] ?? ''}</span>
                            }
                          </td>
                        ))}
                      </tr>
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
