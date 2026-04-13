import { useState, useRef } from 'react';
import { Download, Database } from 'lucide-react';
import api from '@/lib/api';
import { showDownloadToast } from '@/components/DownloadToast';

function formatProgress(loaded: number, total: number | undefined): string {
  const mb = (loaded / (1024 * 1024)).toFixed(1);
  if (total && total > 0) {
    const pct = Math.round((loaded / total) * 100);
    const totalMb = (total / (1024 * 1024)).toFixed(1);
    return `Downloading… ${pct}%  (${mb} / ${totalMb} MB)`;
  }
  return `Downloading… ${mb} MB`;
}

export default function ExportData() {
  const [csvLoading, setCsvLoading] = useState(false);
  const [csvMsg, setCsvMsg] = useState('');
  const [csvError, setCsvError] = useState('');
  const [csvProgress, setCsvProgress] = useState('');

  const [dbLoading, setDbLoading] = useState(false);
  const [dbMsg, setDbMsg] = useState('');
  const [dbError, setDbError] = useState('');
  const [dbProgress, setDbProgress] = useState('');

  // Abort controllers for cancellation
  const csvAbort = useRef<AbortController | null>(null);
  const dbAbort = useRef<AbortController | null>(null);

  const handleDownloadCsv = async () => {
    setCsvMsg(''); setCsvError(''); setCsvProgress('Preparing…');
    setCsvLoading(true);
    csvAbort.current = new AbortController();
    try {
      const res = await api.get('/backup/export/csv', { 
        responseType: 'blob',
        timeout: 0,
        signal: csvAbort.current.signal,
        onDownloadProgress: (evt) => {
          setCsvProgress(formatProgress(evt.loaded, evt.total));
        },
      });
      setCsvProgress('');
      const today = new Date().toISOString().slice(0, 10);
      const defaultName = `cops_full_backup_${today}.zip`;

      try {
        const { save } = await import('@tauri-apps/plugin-dialog');
        const { writeFile } = await import('@tauri-apps/plugin-fs');
        const savePath = await save({ 
          title: 'Save CSV Backup (Includes all modules e.g. BR/DR)', 
          defaultPath: defaultName, 
          filters: [{ name: 'ZIP', extensions: ['zip'] }] 
        });
        
        if (savePath) {
          setCsvProgress('Writing to disk…');
          const arrayBuf = await (res.data as Blob).arrayBuffer();
          await writeFile(savePath, new Uint8Array(arrayBuf));
          setCsvProgress('');
          setCsvMsg(`Backup saved successfully.`);
          showDownloadToast(`Backup saved to ${savePath}`);
        } else {
          setCsvMsg('Save cancelled.');
        }
      } catch (fsErr) {
        if (String(fsErr).includes('plugin-dialog') || String(fsErr).includes('__TAURI_IPC__')) {
          // Fallback only if Tauri environment is missing (running in browser)
          const url = window.URL.createObjectURL(new Blob([res.data], { type: 'application/zip' }));
          const a = document.createElement('a');
          a.href = url; a.download = defaultName; a.click();
          window.URL.revokeObjectURL(url);
          setCsvMsg(`Downloaded successfully.`);
          showDownloadToast(`Downloaded as ${defaultName}`);
        } else {
          throw new Error(`Disk write failed: ${fsErr}`);
        }
      }
    } catch (err: any) {
      if (err?.name === 'CanceledError' || err?.code === 'ERR_CANCELED') {
        setCsvMsg('Download cancelled.');
        setCsvProgress('');
        return;
      }
      // In blob responseType, error data is wrapped in a blob
      let errMsg = 'Download failed.';
      if (err.response?.data instanceof Blob) {
        const text = await err.response.data.text();
        try { errMsg = JSON.parse(text).detail || errMsg; } catch { errMsg = text; }
      } else {
        errMsg = err.response?.data?.detail || err.message;
      }
      setCsvError(errMsg);
      setCsvProgress('');
    } finally {
      setCsvLoading(false);
      csvAbort.current = null;
    }
  };

  const handleDownloadDb = async () => {
    setDbMsg(''); setDbError(''); setDbProgress('Preparing…');
    setDbLoading(true);
    dbAbort.current = new AbortController();
    try {
      const res = await api.get('/backup/export/db', { 
        responseType: 'blob',
        timeout: 0,
        signal: dbAbort.current.signal,
        onDownloadProgress: (evt) => {
          setDbProgress(formatProgress(evt.loaded, evt.total));
        },
      });
      setDbProgress('');
      const today = new Date().toISOString().slice(0, 10);
      const defaultName = `cops_fulldb_${today}.db`;

      try {
        const { save } = await import('@tauri-apps/plugin-dialog');
        const { writeFile } = await import('@tauri-apps/plugin-fs');
        const savePath = await save({ 
          title: 'Save Database Backup (Includes all modules e.g. BR/DR)', 
          defaultPath: defaultName, 
          filters: [{ name: 'Database', extensions: ['db'] }] 
        });
        
        if (savePath) {
          setDbProgress('Writing to disk…');
          const arrayBuf = await (res.data as Blob).arrayBuffer();
          await writeFile(savePath, new Uint8Array(arrayBuf));
          setDbProgress('');
          setDbMsg(`Database saved successfully.`);
          showDownloadToast(`Database saved to ${savePath}`);
        } else {
          setDbMsg('Save cancelled.');
        }
      } catch (fsErr) {
        if (String(fsErr).includes('plugin-dialog') || String(fsErr).includes('__TAURI_IPC__')) {
          const url = window.URL.createObjectURL(new Blob([res.data], { type: 'application/octet-stream' }));
          const a = document.createElement('a');
          a.href = url; a.download = defaultName; a.click();
          window.URL.revokeObjectURL(url);
          setDbMsg(`Downloaded successfully.`);
          showDownloadToast(`Downloaded as ${defaultName}`);
        } else {
          throw new Error(`Disk write failed: ${fsErr}`);
        }
      }
    } catch (err: any) {
      if (err?.name === 'CanceledError' || err?.code === 'ERR_CANCELED') {
        setDbMsg('Download cancelled.');
        setDbProgress('');
        return;
      }
      let errMsg = 'Download failed.';
      if (err.response?.data instanceof Blob) {
        const text = await err.response.data.text();
        try { errMsg = JSON.parse(text).detail || errMsg; } catch { errMsg = text; }
      } else {
        errMsg = err.response?.data?.detail || err.message;
      }
      setDbError(errMsg);
      setDbProgress('');
    } finally {
      setDbLoading(false);
      dbAbort.current = null;
    }
  };

  return (
    <div className="max-w-lg mx-auto py-6 space-y-6">
      <h1 className="text-lg font-bold text-slate-800">Download Backup</h1>

      {/* ── Full SQLite DB (recommended) ── */}
      <div className="border border-slate-200 rounded-lg p-4 space-y-2 bg-white">
        <div className="flex items-center gap-2">
          <Database size={16} className="text-blue-600 shrink-0" />
          <span className="text-sm font-semibold text-slate-800">Full Database Backup <span className="text-xs font-normal text-blue-600 ml-1">Recommended</span></span>
        </div>
        <p className="text-xs text-slate-500">
          Complete snapshot of everything — all OS cases, items, users, shift settings,
          print template headings, baggage rules, statutes, all master tables.
          Uploading this in the admin panel restores the app exactly as it is now on any machine.
        </p>
        <div className="flex items-center gap-3">
          <button
            type="button"
            disabled={dbLoading}
            onClick={handleDownloadDb}
            className="flex items-center gap-2 px-4 py-2 text-sm rounded-lg bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-60"
          >
            <Database size={14} />
            {dbLoading ? (dbProgress || 'Preparing…') : 'Download SQLite Database (.db)'}
          </button>
          {dbLoading && (
            <button
              type="button"
              onClick={() => dbAbort.current?.abort()}
              className="px-3 py-2 text-xs rounded-lg border border-red-300 text-red-600 hover:bg-red-50"
            >
              Cancel
            </button>
          )}
        </div>
        {/* Progress bar */}
        {dbLoading && dbProgress && dbProgress.includes('%') && (
          <div className="w-full bg-slate-200 rounded-full h-1.5 overflow-hidden">
            <div
              className="bg-blue-500 h-1.5 rounded-full transition-all duration-300"
              style={{ width: dbProgress.match(/(\d+)%/)?.[1] + '%' }}
            />
          </div>
        )}
        {dbError && <p className="text-xs text-red-600">{dbError}</p>}
        {dbMsg   && <p className="text-xs text-emerald-700">{dbMsg}</p>}
      </div>

      {/* ── CSV ZIP (cases only) ── */}
      <div className="border border-slate-200 rounded-lg p-4 space-y-2 bg-white">
        <div className="flex items-center gap-2">
          <Download size={16} className="text-slate-500 shrink-0" />
          <span className="text-sm font-semibold text-slate-800">OS Cases Only (CSV ZIP)</span>
        </div>
        <p className="text-xs text-slate-500">
          Exports <strong>cops_master.csv</strong> + <strong>cops_items.csv</strong> — OS cases and
          items only. Does not include users, settings, or print template headings.
          Use this for selective migration or sharing data with another system.
        </p>
        <div className="flex items-center gap-3">
          <button
            type="button"
            disabled={csvLoading}
            onClick={handleDownloadCsv}
            className="flex items-center gap-2 px-4 py-2 text-sm rounded-lg bg-emerald-600 text-white hover:bg-emerald-700 disabled:opacity-60"
          >
            <Download size={14} />
            {csvLoading ? (csvProgress || 'Preparing…') : 'Download Backup ZIP'}
          </button>
          {csvLoading && (
            <button
              type="button"
              onClick={() => csvAbort.current?.abort()}
              className="px-3 py-2 text-xs rounded-lg border border-red-300 text-red-600 hover:bg-red-50"
            >
              Cancel
            </button>
          )}
        </div>
        {/* Progress bar */}
        {csvLoading && csvProgress && csvProgress.includes('%') && (
          <div className="w-full bg-slate-200 rounded-full h-1.5 overflow-hidden">
            <div
              className="bg-emerald-500 h-1.5 rounded-full transition-all duration-300"
              style={{ width: csvProgress.match(/(\d+)%/)?.[1] + '%' }}
            />
          </div>
        )}
        {csvError && <p className="text-xs text-red-600">{csvError}</p>}
        {csvMsg   && <p className="text-xs text-emerald-700">{csvMsg}</p>}
      </div>
    </div>
  );
}
