import { useEffect, useState } from 'react';
import { useLocation } from 'react-router-dom';
import { AlertTriangle, Download, X } from 'lucide-react';
import api from '@/lib/api';

/**
 * A once-a-month reminder to take a backup off this network.
 *
 * The automatic backups all live on the office LAN. That covers a failed disk;
 * it does not cover a fire, a theft, or ransomware, which take every copy in
 * the building at once. Someone has to carry a file out on a stick.
 *
 * It used to be a strip along the bottom of every screen, every day. A warning
 * an officer sees daily is a warning they stop reading, and it was competing
 * with the work. Now it appears once a month, as a dialog that has to be
 * answered, and then stays quiet:
 *
 *   - shown on the 1st of a month, or the first time the app is opened after it
 *   - dismissed, or a backup taken, and it does not return until the next month
 *
 * The month it was last settled is kept in localStorage, so closing the app
 * does not bring it back.
 */
const SEEN_KEY = 'cops_archive_reminder_month';
const monthKey = (d = new Date()) => `${d.getFullYear()}-${d.getMonth() + 1}`;

export default function ArchiveReminder() {
  const [state, setState] = useState<string | null>(null);
  const [days, setDays] = useState<number | null>(null);
  const [open, setOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const location = useLocation();

  useEffect(() => {
    // Not on the print view — it would land in the printed page.
    if (location.pathname.includes('/print')) return;
    let cancelled = false;

    const check = async () => {
      const token = localStorage.getItem('cops_token');
      if (!token) return;                                  // nobody signed in
      if (localStorage.getItem(SEEN_KEY) === monthKey()) return;   // already settled this month
      try {
        const res = await fetch(`${api.defaults.baseURL}/backup/archive/status`, {
          headers: { Authorization: `Bearer ${token}` },
        });
        if (!res.ok || cancelled) return;
        const data = await res.json();
        if (cancelled) return;
        if (data.state === 'never' || data.state === 'overdue' || data.state === 'due') {
          setState(data.state);
          setDays(data.days_since ?? null);
          setOpen(true);
        }
      } catch {
        // Never surfaced. This is a nudge; if the check cannot run the officer
        // should not be stopped by it.
      }
    };

    const first = setTimeout(check, 4000);
    return () => { cancelled = true; clearTimeout(first); };
  }, [location.pathname]);

  /** Settle it for this month, however the officer chose to settle it. */
  const settle = () => {
    localStorage.setItem(SEEN_KEY, monthKey());
    setOpen(false);
  };

  const download = async () => {
    setSaving(true);
    try {
      const token = localStorage.getItem('cops_token');
      const res = await fetch(`${api.defaults.baseURL}/backup/archive/download`, {
        headers: { Authorization: `Bearer ${token}` },
      });
      if (!res.ok) throw new Error(String(res.status));
      const blob = await res.blob();
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `cops_backup_${new Date().toISOString().slice(0, 10)}.cops`;
      document.body.appendChild(a);
      a.click();
      a.remove();
      URL.revokeObjectURL(url);
      setSaved(true);
      // Taking the backup settles the month too — that was the point of asking.
      localStorage.setItem(SEEN_KEY, monthKey());
      setTimeout(() => setOpen(false), 1500);
    } catch {
      setSaved(false);
    } finally {
      setSaving(false);
    }
  };

  if (!open) return null;
  const overdue = state === 'never' || state === 'overdue';

  return (
    <div className="fixed inset-0 z-[9995] flex items-center justify-center bg-slate-900/60 p-4 print:hidden">
      <div className="w-full max-w-md rounded-xl bg-white shadow-xl border border-slate-200 overflow-hidden">
        <div className={`flex items-center gap-2 px-5 py-3 border-b
                         ${overdue ? 'bg-red-50 border-red-200' : 'bg-amber-50 border-amber-200'}`}>
          <AlertTriangle size={18} className={overdue ? 'text-red-600' : 'text-amber-600'} />
          <h2 className={`text-sm font-semibold ${overdue ? 'text-red-800' : 'text-amber-900'}`}>
            Monthly backup
          </h2>
        </div>

        <div className="px-5 py-4 space-y-3">
          <p className="text-sm text-slate-700 leading-relaxed">
            {state === 'never'
              ? 'No backup has ever been saved off this network.'
              : `The last backup taken out of the office was ${days} days ago.`}
            {' '}
            The automatic copies all sit on this network, so a fire or a theft
            would take every one of them at the same time.
          </p>
          <p className="text-sm text-slate-700 leading-relaxed">
            Save a copy now and keep it on a pen drive or an external disk, away
            from this room. The file is encrypted, so it is safe to carry.
          </p>
          <div className="rounded-lg border border-slate-200 bg-slate-50 px-3 py-2">
            <p className="text-xs text-slate-600 leading-relaxed">
              <span className="font-semibold text-slate-700">One file holds everything.</span>{' '}
              Each backup is a complete copy of the whole register from the very
              first case up to today — not just the month gone by. The newest file
              replaces the one before it, so there is no need to keep the old ones.
            </p>
          </div>
          {saved && (
            <p className="text-xs font-medium text-emerald-700">
              Saved. Copy it onto the drive before putting it away.
            </p>
          )}
        </div>

        <div className="flex items-center justify-end gap-2 px-5 py-3 bg-slate-50 border-t border-slate-100">
          <button
            type="button"
            onClick={settle}
            className="flex items-center gap-1.5 px-3 py-2 text-xs rounded-lg text-slate-600 hover:bg-slate-100 font-medium"
          >
            <X size={14} /> Not now
          </button>
          <button
            type="button"
            disabled={saving}
            onClick={download}
            className="flex items-center gap-2 px-4 py-2 text-xs rounded-lg bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-60 font-semibold"
          >
            <Download size={14} /> {saving ? 'Preparing…' : 'Download backup'}
          </button>
        </div>

        <p className="px-5 pb-4 text-[11px] text-slate-400">
          You will not be asked again until next month.
        </p>
      </div>
    </div>
  );
}
