import { useState, useRef, useEffect } from 'react';
import { Download, RefreshCw, AlertTriangle } from 'lucide-react';
import api from '@/lib/api';
import { saveBlob } from '@/lib/saveFile';
import { showDownloadToast } from '@/components/DownloadToast';
import SupportContact from '@/components/SupportContact';
import { useAppMode } from '@/hooks/useAppMode';

interface BackupHealthDest {
  path: string; reachable: boolean; off_machine: boolean; last_ok: string | null;
}
interface BackupHealth {
  destinations: BackupHealthDest[];
  any_off_machine: boolean;
  healthy: boolean;
  last_success: string | null;
  refusing: string | null;
}

/**
 * One button: take the backup.
 *
 * This page used to explain what a backup contains and how much smaller it is
 * than the database, above a button that exported only the O.S. cases as CSV.
 * An officer who wants a backup wants the file, not the explanation, and the
 * button they were given was not the backup.
 *
 * So: the whole archive — every register, every setting, every user — in one
 * encrypted file, saved wherever they choose. Nothing else on the page.
 */
export default function ExportData() {
  const { isProd } = useAppMode();
  const [busy, setBusy] = useState(false);
  const [percent, setPercent] = useState<number | null>(null);
  const [downloaded, setDownloaded] = useState('');
  const [done, setDone] = useState('');
  // The friendly message the officer sees, and — only in development — the raw
  // technical detail. The detail is never shown in production: it helps no
  // officer, and could tell a probing attacker how the machine is built.
  const [error, setError] = useState('');
  const [errorDetail, setErrorDetail] = useState('');
  const abort = useRef<AbortController | null>(null);

  // "Sync now" — push a fresh backup to the slave machines without waiting out
  // the rest of the half-hour interval. Separate state from the download above so
  // one running never blanks the other's message.
  const [syncing, setSyncing] = useState(false);
  const [syncMsg, setSyncMsg] = useState<{ ok: boolean; lines: string[] } | null>(null);

  // Backup health, so a slave machine that has quietly stopped receiving copies
  // is seen HERE, on the page officers use, not only in the admin panel. An
  // unreachable off-machine copy is the failure that is otherwise found on the
  // day of a restore. Refreshed after a sync, so the dots move when they act.
  const [health, setHealth] = useState<BackupHealth | null>(null);
  const loadHealth = () => {
    api.get('/backup/auto/status')
      .then(r => setHealth(r.data as BackupHealth))
      .catch(() => { /* a nudge, never a blocker */ });
  };
  useEffect(loadHealth, []);

  // The off-machine copies that are currently unreachable — the ones worth
  // shouting about. The local copy always being present is not news.
  const unreachableOffMachine = (health?.destinations ?? [])
    .filter(d => d.off_machine && !d.reachable);
  const noOffMachineCopy = health != null && !health.any_off_machine;

  const syncNow = async () => {
    setSyncMsg(null);
    setSyncing(true);
    try {
      // The reply is deliberately minimal — counts only, no folder paths (see
      // sync_now). The per-machine detail comes from the health banner above.
      const res = await api.post('/backup/sync', {}, { timeout: 0 });
      const o = res.data as {
        ok: boolean; skipped: boolean; refused: boolean;
        copied: number; total: number; reason: string;
      };
      const reason = (o.reason || '').toLowerCase();
      if (o.refused) {
        setSyncMsg({ ok: false, lines: [
          'Backup refused as a safeguard — a possible loss of records was detected.',
          'Ask the administrator to check it from the admin panel.',
        ] });
      } else if (reason.includes('already running')) {
        setSyncMsg({ ok: false, lines: [
          'A backup is already running. It will finish on its own — try again in a moment.',
        ] });
      } else if (reason.includes('a moment ago')) {
        setSyncMsg({ ok: true, lines: ['A backup was just taken a moment ago.'] });
      } else if (reason.includes('no backup folders') || o.total === 0) {
        setSyncMsg({ ok: false, lines: [
          'No other backup machines are configured yet.',
          'Ask the administrator to set the backup folders in the admin panel.',
        ] });
      } else if (o.copied === o.total) {
        setSyncMsg({ ok: true, lines: [`Backup saved to all ${o.total} location(s).`] });
      } else {
        setSyncMsg({ ok: false, lines: [
          `Backup saved to ${o.copied} of ${o.total} location(s).`,
          'One machine could not be reached — see the details above, or switch it on and try again.',
        ] });
      }
    } catch (err: unknown) {
      const e = err as { response?: { data?: { detail?: string } }; message?: string };
      setSyncMsg({ ok: false, lines: [e?.response?.data?.detail || e?.message || 'The sync could not be completed.'] });
    } finally {
      setSyncing(false);
      loadHealth();   // reflect the just-changed reachability in the dots
    }
  };

  const download = async () => {
    setDone(''); setError(''); setErrorDetail(''); setPercent(null); setDownloaded('');
    setBusy(true);
    abort.current = new AbortController();
    try {
      const res = await api.get('/backup/archive/download', {
        responseType: 'blob',
        timeout: 0,                       // a large register takes as long as it takes
        signal: abort.current.signal,
        onDownloadProgress: (evt) => {
          const mb = evt.loaded / (1024 * 1024);
          setDownloaded(`${mb.toFixed(1)} MB`);
          // The server cannot always say how large the file will be; without a
          // total there is still movement to show.
          setPercent(evt.total ? Math.round((evt.loaded / evt.total) * 100) : null);
        },
      });

      const name = `cops_backup_${new Date().toISOString().slice(0, 10)}.cops`;
      const saved = await saveBlob(res.data as Blob, name, {
        title: 'Save the backup', extensions: ['cops'],
      });
      if (saved.cancelled) { setDone(''); return; }

      setDone(`Saved to ${saved.path}`);
      showDownloadToast(`Backup saved to ${saved.path}`);
      // The monthly reminder counts a backup as taken only when a file exists.
      localStorage.setItem('cops_archive_reminder_done_month',
        `${new Date().getFullYear()}-${new Date().getMonth() + 1}`);
      localStorage.removeItem('cops_archive_reminder_snoozed_until');
    } catch (err: unknown) {
      const e = err as { name?: string; code?: string; message?: string;
                         response?: { data?: unknown } };
      if (e?.name === 'CanceledError' || e?.code === 'ERR_CANCELED') return;
      let detail = e?.message || 'unknown error';
      if (e?.response?.data instanceof Blob) {
        const text = await (e.response.data as Blob).text();
        try { detail = JSON.parse(text).detail || text; } catch { detail = text || detail; }
      }
      setError('The backup could not be saved. Please try again.');
      setErrorDetail(detail);
    } finally {
      setBusy(false);
      setPercent(null);
      abort.current = null;
    }
  };

  return (
    <div className="max-w-lg mx-auto py-10 space-y-4">
      {(unreachableOffMachine.length > 0 || noOffMachineCopy || health?.refusing) && (
        <div className="rounded-lg border border-red-200 bg-red-50 p-3 flex items-start gap-2">
          <AlertTriangle size={18} className="text-red-600 shrink-0 mt-0.5" />
          <div className="text-xs text-red-800 space-y-1">
            {health?.refusing && (
              <>
                <p className="font-semibold">
                  Backups are being held back as a safeguard — the records look smaller than the
                  last saved copy, which can mean data was lost.
                </p>
                <p>Do not close the app. Tell the administrator to check this straight away.</p>
                <SupportContact className="text-red-700/90" />
              </>
            )}
            {unreachableOffMachine.length > 0 && (
              <>
                <p className="font-semibold">
                  A backup machine can&rsquo;t be reached, so its copy is not up to date.
                </p>
                {unreachableOffMachine.map(d => (
                  <p key={d.path} className="font-mono break-all">
                    {d.path}{d.last_ok ? ` · last copied ${new Date(d.last_ok).toLocaleDateString()}` : ' · never copied'}
                  </p>
                ))}
                <p>Switch that PC on and press <strong>Sync now</strong>. If it stays red, tell the administrator.</p>
              </>
            )}
            {noOffMachineCopy && (
              <p className="font-semibold">
                Backups are only being kept on this computer. Ask the administrator to add a backup
                folder on another PC, so a copy survives if this machine fails.
              </p>
            )}
          </div>
        </div>
      )}

      <div className="flex items-center gap-3">
        <button
          type="button"
          disabled={busy}
          onClick={download}
          className="flex items-center gap-2 px-5 py-2.5 text-sm font-semibold rounded-lg
                     bg-emerald-600 text-white hover:bg-emerald-700 disabled:opacity-60"
        >
          <Download size={16} />
          {busy ? 'Preparing…' : 'Download backup'}
        </button>
        {busy && (
          <button
            type="button"
            onClick={() => abort.current?.abort()}
            className="px-3 py-2 text-xs rounded-lg border border-slate-300 text-slate-600 hover:bg-slate-50"
          >
            Cancel
          </button>
        )}
        <button
          type="button"
          disabled={syncing}
          onClick={syncNow}
          title="Push a fresh backup to the slave machines now, without waiting for the automatic 30-minute sync"
          className="flex items-center gap-2 px-5 py-2.5 text-sm font-semibold rounded-lg
                     border border-slate-300 text-slate-700 hover:bg-slate-50 disabled:opacity-60"
        >
          <RefreshCw size={16} className={syncing ? 'animate-spin' : ''} />
          {syncing ? 'Syncing…' : 'Sync now'}
        </button>
      </div>

      <div className="text-xs text-slate-500 space-y-1.5">
        <p>
          <span className="font-medium text-slate-700">Download backup</span> saves one encrypted
          file to this computer or a USB drive — carry it out of the office so a copy survives a
          fire or theft. This is the copy the monthly reminder asks for.
        </p>
        <p>
          <span className="font-medium text-slate-700">Sync now</span> pushes the latest backup to
          the other (slave) PCs on the network right away — the same thing that happens
          automatically every 30 minutes. It keeps the office copies current; it does not leave the
          building.
        </p>
      </div>

      {syncMsg && (
        <div className={`text-xs rounded-lg border p-3 space-y-0.5 ${
          syncMsg.ok ? 'bg-emerald-50 border-emerald-200 text-emerald-800'
                     : 'bg-amber-50 border-amber-200 text-amber-800'}`}>
          {syncMsg.lines.map((l, i) => (
            <p key={i} className={i === 0 ? 'font-medium' : 'font-mono'}>{l}</p>
          ))}
          {!syncMsg.ok && <SupportContact className="text-amber-700/90 pt-1" />}
        </div>
      )}

      {busy && (
        <div className="space-y-1">
          <div className="w-full bg-slate-200 rounded-full h-2 overflow-hidden">
            {percent === null ? (
              // No total to measure against — a moving bar, so the officer can
              // see it is working rather than wonder whether it has hung.
              <div className="bg-emerald-500 h-2 w-1/3 rounded-full animate-pulse" />
            ) : (
              <div
                className="bg-emerald-500 h-2 rounded-full transition-all duration-300"
                style={{ width: `${percent}%` }}
              />
            )}
          </div>
          <p className="text-xs text-slate-500">
            {percent !== null ? `${percent}% · ` : ''}{downloaded}
          </p>
        </div>
      )}

      {done  && <p className="text-xs text-emerald-700">{done}</p>}
      {error && (
        <div className="text-xs rounded-lg border border-red-200 bg-red-50 p-3 space-y-1.5 text-red-800">
          <p className="font-medium">{error}</p>
          {!isProd && errorDetail && (
            <p className="font-mono text-[11px] text-red-600 break-all">{errorDetail}</p>
          )}
          <SupportContact className="text-red-700/90" />
        </div>
      )}
    </div>
  );
}
