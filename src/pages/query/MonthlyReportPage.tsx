import { useState } from 'react';
import { Calendar, Download, FileDown, Loader2, FileText } from 'lucide-react';
import api from '../../lib/api';
import { showDownloadToast } from '@/components/DownloadToast';

const MONTHS = [
  'January', 'February', 'March', 'April', 'May', 'June',
  'July', 'August', 'September', 'October', 'November', 'December'
];

const CURRENT_YEAR = new Date().getFullYear();
const YEARS = Array.from({ length: CURRENT_YEAR - 1999 }, (_, i) => CURRENT_YEAR - i);

interface MonthlyReportRow {
  os_no: string;
  os_date: string | null;
  batch_aiu: string | null;
  flt_no: string | null;
  pax_name: string | null;
  nationality: string | null;
  passport_no: string | null;
  address: string | null;
  item_description: string | null;
  tags: string | null;
  quantity: string | null;
  value_in_rs: number;
  oinO_no: string;
  date_of_oinO: string;
  rf_ref: number;
  penalty: number;
  duty_rs: number;
  other_charges: string;
  total: number;
  br_no: string | null;
  br_date: string | null;
  remarks: string | null;
  file_spot: string;
  adjudicated_by_ac_dc: string | null;
  adjudicated_by_jc_adc: string;
  export_import: string;
  column1: string | null;
}

const COLUMNS: { key: keyof MonthlyReportRow; label: string }[] = [
  { key: 'os_no', label: 'OS NO' },
  { key: 'os_date', label: 'OS DATE' },
  { key: 'batch_aiu', label: 'BATCH / AIU' },
  { key: 'flt_no', label: 'FLT NO' },
  { key: 'pax_name', label: 'PAX NAME' },
  { key: 'nationality', label: 'NATIONALITY' },
  { key: 'passport_no', label: 'PASSPORT NO.' },
  { key: 'address', label: 'ADDRESS' },
  { key: 'item_description', label: 'ITEM DESCRIPTION' },
  { key: 'tags', label: 'TAGS' },
  { key: 'quantity', label: 'QUANTITY' },
  { key: 'value_in_rs', label: 'VALUE IN RS.' },
  { key: 'oinO_no', label: 'O-in-O NO' },
  { key: 'date_of_oinO', label: 'DATE OF O-IN-O' },
  { key: 'rf_ref', label: 'RF/R.E.F' },
  { key: 'penalty', label: 'PENALTY' },
  { key: 'duty_rs', label: 'DUTY in RS' },
  { key: 'other_charges', label: 'Other Charges' },
  { key: 'total', label: 'TOTAL' },
  { key: 'br_no', label: 'B.R NO' },
  { key: 'br_date', label: 'B.R Date' },
  { key: 'remarks', label: 'REMARKS' },
  { key: 'file_spot', label: 'FILE / SPOT -ADJUDICATION' },
  { key: 'adjudicated_by_ac_dc', label: 'ADJUDICATED BY AC/DC' },
  { key: 'adjudicated_by_jc_adc', label: 'ADJUDICATED BY JC/ADC' },
  { key: 'export_import', label: 'EXPORT/IMPORT' },
  { key: 'column1', label: 'Confiscation Type' },
];

const NUMERIC_COLS = new Set<keyof MonthlyReportRow>(['value_in_rs', 'rf_ref', 'penalty', 'duty_rs', 'total']);

function fmtDate(d: string | null | undefined): string {
  if (!d) return '';
  try {
    const dt = new Date(d);
    if (isNaN(dt.getTime())) return d;
    return dt.toLocaleDateString('en-GB');
  } catch { return d; }
}

function fmtNum(n: number | null | undefined): string {
  if (n === null || n === undefined || n === 0) return '';
  return n.toLocaleString('en-IN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
}

function cellValue(row: MonthlyReportRow, key: keyof MonthlyReportRow): string {
  const v = row[key];
  if (v === null || v === undefined) return '';
  if (key === 'os_date') return fmtDate(v as string);
  if (NUMERIC_COLS.has(key)) return fmtNum(v as number);
  return String(v);
}

export default function MonthlyReportPage() {
  const now = new Date();
  const [month, setMonth] = useState<number>(now.getMonth() + 1);
  const [year, setYear] = useState<number>(now.getFullYear());
  const [loading, setLoading] = useState(false);
  const [rows, setRows] = useState<MonthlyReportRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const fetchReport = async () => {
    setLoading(true);
    setError(null);
    setRows(null);
    try {
      const res = await api.get('/os-query/monthly-report', { params: { month, year } });
      setRows(res.data || []);
    } catch (e: unknown) {
      const msg = (e as { response?: { data?: { detail?: string } } })?.response?.data?.detail || 'Failed to load report.';
      setError(msg);
    } finally {
      setLoading(false);
    }
  };

  const buildCsvContent = (): string => {
    if (!rows) return '';
    const escape = (v: string) => `"${v.replace(/"/g, '""')}"`;
    const headers = COLUMNS.map(c => escape(c.label)).join(',');
    const dataRows = rows.map(row =>
      COLUMNS.map(c => {
        const val = cellValue(row, c.key);
        return escape(val);
      }).join(',')
    );
    // Add BOM for Excel UTF-8 compatibility
    return '\uFEFF' + [headers, ...dataRows].join('\r\n');
  };

  const downloadCSV = async () => {
    const csvContent = buildCsvContent();
    const monthName = MONTHS[month - 1];
    const defaultName = `Monthly_Report_${monthName}_${year}.csv`;

    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const { writeTextFile } = await import('@tauri-apps/plugin-fs');
      const savePath = await save({
        title: 'Save Monthly Report',
        defaultPath: defaultName,
        filters: [{ name: 'CSV Files', extensions: ['csv'] }],
      });
      if (savePath) {
        await writeTextFile(savePath, csvContent);
        showDownloadToast(`Report saved to ${savePath}`);
      }
    } catch {
      const blob = new Blob([csvContent], { type: 'text/csv;charset=utf-8;' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = defaultName;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      setTimeout(() => URL.revokeObjectURL(url), 5000);
      showDownloadToast(`Report downloaded as ${defaultName}`);
    }
  };

  return (
    <div className="space-y-5 max-w-full">
      {/* Header & Controls */}
      <div className="bg-white rounded-xl shadow-sm border border-slate-200 overflow-hidden">
        <div className="bg-slate-50 border-b border-slate-200 p-4">
          <h2 className="text-lg font-bold text-slate-800 flex items-center gap-2">
            <Calendar className="w-5 h-5 text-emerald-500" />
            Monthly OS Register Report
          </h2>
          <p className="text-sm text-slate-500 mt-0.5">
            Generates the monthly seizure register in the standard 27-column format
          </p>
        </div>

        <div className="p-5 flex flex-wrap items-end gap-4">
          <div>
            <label className="block text-xs font-semibold text-slate-600 mb-1">Month</label>
            <select
              value={month}
              onChange={e => setMonth(Number(e.target.value))}
              className="bg-white border border-slate-300 rounded-md text-sm px-3 py-2 text-slate-800 focus:ring-emerald-500 focus:border-emerald-500"
            >
              {MONTHS.map((m, i) => (
                <option key={m} value={i + 1}>{m}</option>
              ))}
            </select>
          </div>

          <div>
            <label className="block text-xs font-semibold text-slate-600 mb-1">Year</label>
            <select
              value={year}
              onChange={e => setYear(Number(e.target.value))}
              className="bg-white border border-slate-300 rounded-md text-sm px-3 py-2 text-slate-800 focus:ring-emerald-500 focus:border-emerald-500"
            >
              {YEARS.map(y => (
                <option key={y} value={y}>{y}</option>
              ))}
            </select>
          </div>

          <button
            onClick={fetchReport}
            disabled={loading}
            className="flex items-center gap-2 px-5 py-2 text-sm font-medium text-white bg-emerald-600 rounded-md hover:bg-emerald-700 disabled:opacity-50 focus:outline-none focus:ring-2 focus:ring-emerald-500"
          >
            {loading ? <Loader2 className="w-4 h-4 animate-spin" /> : <Calendar className="w-4 h-4" />}
            Generate Report
          </button>

          {rows && rows.length > 0 && (
            <button
              onClick={downloadCSV}
              className="flex items-center gap-2 px-5 py-2 text-sm font-medium text-white bg-blue-600 rounded-md hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500"
            >
              <FileDown className="w-4 h-4" />
              Download Excel (CSV)
            </button>
          )}
        </div>
      </div>

      {/* Error */}
      {error && (
        <div className="bg-red-50 border border-red-200 text-red-700 text-sm rounded-lg px-4 py-3">
          <span className="font-semibold">Error:</span> {error}
        </div>
      )}

      {/* Results */}
      {rows !== null && !error && (
        <div className="bg-white rounded-xl shadow-sm border border-slate-200 overflow-hidden">
          <div className="bg-slate-50 border-b border-slate-200 p-4 flex justify-between items-center">
            <h3 className="font-bold text-slate-800 flex items-center gap-2">
              <Download className="w-4 h-4 text-emerald-500" />
              {MONTHS[month - 1]} {year} — Monthly Register
              <span className="ml-2 text-xs font-medium bg-emerald-100 text-emerald-800 px-2 py-0.5 rounded-full">
                {rows.length} case{rows.length !== 1 && 's'}
              </span>
            </h3>
            {rows.length > 0 && (
              <button
                onClick={downloadCSV}
                className="flex items-center gap-1.5 text-xs font-medium text-white bg-emerald-600 border border-emerald-600 px-3 py-1.5 rounded hover:bg-emerald-700 shadow-sm"
              >
                <FileDown className="w-3.5 h-3.5" /> Download CSV
              </button>
            )}
          </div>

          {rows.length === 0 ? (
            <div className="p-12 text-center text-slate-500">
              <FileText className="w-12 h-12 mx-auto text-slate-300 mb-3" />
              <p className="font-medium">No submitted cases found for {MONTHS[month - 1]} {year}.</p>
              <p className="text-xs mt-1 text-slate-400">Only submitted (non-draft) cases appear in this report.</p>
            </div>
          ) : (
            <div className="overflow-auto max-h-[65vh]">
              <table className="min-w-full text-xs border-collapse">
                <thead className="bg-slate-100 sticky top-0 z-10">
                  <tr>
                    <th className="px-2 py-2 text-left font-semibold text-slate-600 border border-slate-200 whitespace-nowrap">#</th>
                    {COLUMNS.map(c => (
                      <th
                        key={c.key}
                        className="px-2 py-2 text-left font-semibold text-slate-600 border border-slate-200 whitespace-nowrap uppercase tracking-wide"
                        style={{ fontSize: '10px' }}
                      >
                        {c.label}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {rows.map((row, idx) => (
                    <tr
                      key={`${row.os_no}-${idx}`}
                      className={idx % 2 === 0 ? 'bg-white hover:bg-emerald-50/40' : 'bg-slate-50/70 hover:bg-emerald-50/40'}
                    >
                      <td className="px-2 py-1.5 text-slate-400 border border-slate-100 font-mono">{idx + 1}</td>
                      {COLUMNS.map(c => {
                        const val = cellValue(row, c.key);
                        const isNumeric = NUMERIC_COLS.has(c.key);
                        const isBold = c.key === 'os_no';
                        const isWide = ['address', 'item_description', 'tags', 'quantity', 'remarks'].includes(c.key);
                        return (
                          <td
                            key={c.key}
                            className={`px-2 py-1.5 border border-slate-100 ${isNumeric ? 'text-right font-mono' : ''} ${isBold ? 'font-semibold text-slate-800' : 'text-slate-700'}`}
                            style={isWide ? { maxWidth: '200px', whiteSpace: 'normal', wordBreak: 'break-word' } : { whiteSpace: 'nowrap' }}
                            title={isWide ? val : undefined}
                          >
                            {val || <span className="text-slate-300">—</span>}
                          </td>
                        );
                      })}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
