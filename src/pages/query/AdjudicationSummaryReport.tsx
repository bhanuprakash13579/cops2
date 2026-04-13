import { useState } from 'react';
import { Users, FileDown, RefreshCw } from 'lucide-react';
import api from '@/lib/api';
import DatePicker from '@/components/DatePicker';
import { showDownloadToast } from '@/components/DownloadToast';

export default function AdjudicationSummaryReport() {
  const [fromDate, setFromDate] = useState('');
  const [toDate, setToDate] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  const generate = async () => {
    if (!fromDate || !toDate) {
      setError('Please select both From Date and To Date.');
      return;
    }
    if (fromDate > toDate) {
      setError('From Date cannot be after To Date.');
      return;
    }
    setError('');
    setLoading(true);

    try {
      const res = await api.post(
        '/backup/adjudication-summary-pdf',
        { from_date: fromDate, to_date: toDate },
        { responseType: 'blob' },
      );

      const fromLabel = fromDate.replace(/-/g, '');
      const toLabel   = toDate.replace(/-/g, '');
      const filename  = `adjn_summary_${fromLabel}_to_${toLabel}.pdf`;

      try {
        const { save } = await import('@tauri-apps/plugin-dialog');
        const { writeFile } = await import('@tauri-apps/plugin-fs');
        const savePath = await save({
          title: 'Save Adjudication Summary PDF',
          defaultPath: filename,
          filters: [{ name: 'PDF Document', extensions: ['pdf'] }],
        });
        if (savePath) {
          const arrayBuf = await (res.data as Blob).arrayBuffer();
          await writeFile(savePath, new Uint8Array(arrayBuf));
          showDownloadToast(`Report saved to ${savePath}`);
        }
      } catch {
        // Fallback for browser / non-Tauri
        const url = URL.createObjectURL(new Blob([res.data], { type: 'application/pdf' }));
        const a = document.createElement('a');
        a.href = url;
        a.download = filename;
        a.click();
        URL.revokeObjectURL(url);
        showDownloadToast(`Downloaded as ${filename}`);
      }
    } catch (err: any) {
      const detail = err.response?.data
        ? await (err.response.data as Blob).text().then(t => {
            try { return JSON.parse(t).detail; } catch { return t; }
          }).catch(() => 'Failed to generate report.')
        : 'Failed to generate report.';
      setError(detail);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="max-w-xl mx-auto py-6 space-y-6">

      {/* Header */}
      <div>
        <h1 className="text-lg font-bold text-slate-800 flex items-center gap-2">
          <Users size={20} className="text-emerald-600" />
          Officer Performance Summary
        </h1>
        <p className="text-xs text-slate-500 mt-1">
          Generates a PDF report summarising each adjudicating officer's activity
          — total value adjudicated, duty &amp; fines levied — for the selected period.
        </p>
      </div>

      {/* Controls */}
      <div className="bg-white border border-slate-200 rounded-xl p-5 space-y-5">
        <p className="text-xs font-semibold text-slate-600 uppercase tracking-wide">
          Select Date Range
        </p>

        <div className="grid grid-cols-2 gap-4">
          <div>
            <label className="block text-xs text-slate-500 mb-1">
              From Date <span className="text-red-400">*</span>
            </label>
            <DatePicker
              value={fromDate}
              onChange={v => { setFromDate(v); setError(''); }}
              inputClassName="input-field"
              placeholder="dd/mm/yyyy"
            />
          </div>
          <div>
            <label className="block text-xs text-slate-500 mb-1">
              To Date <span className="text-red-400">*</span>
            </label>
            <DatePicker
              value={toDate}
              onChange={v => { setToDate(v); setError(''); }}
              inputClassName="input-field"
              placeholder="dd/mm/yyyy"
            />
          </div>
        </div>

        {error && (
          <p className="text-xs text-red-600 bg-red-50 border border-red-200 rounded-lg px-3 py-2">
            {error}
          </p>
        )}

        <button
          type="button"
          onClick={generate}
          disabled={loading}
          className="flex items-center gap-2 px-5 py-2.5 text-sm rounded-lg bg-emerald-600 text-white hover:bg-emerald-700 disabled:opacity-50 transition-colors"
        >
          {loading
            ? <><RefreshCw size={14} className="animate-spin" /> Generating PDF…</>
            : <><FileDown size={14} /> Generate &amp; Download PDF</>
          }
        </button>
      </div>

      {/* Info box */}
      <div className="bg-slate-50 border border-slate-200 rounded-xl p-4 space-y-2">
        <p className="text-xs font-semibold text-slate-600">What's included in the report</p>
        <ul className="text-xs text-slate-500 space-y-1 list-disc list-inside">
          <li>Only cases adjudicated within the selected period (by adjudication date)</li>
          <li>One row per adjudicating officer, aggregated across all their cases</li>
          <li>Columns: Total OS Value, Dutiable Value, Redeemed / Re-export / Confiscated Value</li>
          <li>Duty Levied, Redemption Fine (R.F.), Re-export Fine (R.E.F.), Personal Penalty</li>
          <li>Grand total row at the bottom</li>
          <li>All amounts in Indian Rupees, rounded to nearest rupee</li>
        </ul>
      </div>

    </div>
  );
}
