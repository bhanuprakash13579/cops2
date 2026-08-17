import { useEffect, useState } from 'react';
import { useLocation } from 'react-router-dom';
import { AlertTriangle, Download, X } from 'lucide-react';
import api from '@/lib/api';
import { saveBlob } from '@/lib/saveFile';

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
 *   - shown the first time the app is opened in a new month, whether or not
 *     anyone has booked a case: it runs on launch, not on any piece of work
 *   - taking the backup settles the month, and it stays quiet until the next one
 *   - "Not now" only defers it for three days
 *
 * That last distinction matters. Dismissing used to silence it for the whole
 * month, so an officer who was busy on the 1st simply never saw it again and the
 * backup never happened — the reminder quietly defeated itself. Deferring now
 * brings it back, and only actually saving a file stops it.
 */
const DONE_KEY   = 'cops_archive_reminder_done_month';   // month a backup was taken
const SNOOZE_KEY = 'cops_archive_reminder_snoozed_until'; // ms timestamp
const SNOOZE_DAYS = 3;
const monthKey = (d = new Date()) => `${d.getFullYear()}-${d.getMonth() + 1}`;

export default function ArchiveReminder() {
  const [state, setState] = useState<string | null>(null);
  const [days, setDays] = useState<number | null>(null);
  const [open, setOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const location = useLocation();

  useEffect(() => {
    // Not while the officer is in the middle of something.
    //
    // This is a dialog across the whole window, so it takes the clicks meant for
    // whatever is underneath. Appearing over a half-typed case — or over the
    // adjudication order — it stops the work it interrupted until it is answered,
    // and a case is not a thing to be interrupted in the middle of.
    //
    // It waits for a list or a dashboard, where there is nothing to lose. The
    // print view is excluded too: it would land in the printed page.
    const busy = ['/print', '/new', '/edit', '/view', '/case/', '/adjudicat', '/offline-adjudication', '/dcr'];
    if (busy.some(part => location.pathname.includes(part))) return;
    let cancelled = false;

    const check = async () => {
      const token = localStorage.getItem('cops_token');
      if (!token) return;                                  // nobody signed in
      // Settled for this month only by actually taking a backup.
      if (localStorage.getItem(DONE_KEY) === monthKey()) return;
      // Otherwise honour a short deferral, so it returns rather than vanishing.
      const until = Number(localStorage.getItem(SNOOZE_KEY) || 0);
      if (until && Date.now() < until) return;
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

  /** Defer, not dismiss — it comes back in a few days if no backup is taken. */
  const notNow = () => {
    localStorage.setItem(SNOOZE_KEY, String(Date.now() + SNOOZE_DAYS * 86400_000));
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

      // Through the operating system's Save dialog. Pointing a hidden link at a
      // blob does nothing in the desktop window — no file, no error — so this
      // button appeared to do nothing at all when it was pressed.
      const name = `cops_backup_${new Date().toISOString().slice(0, 10)}.cops`;
      const out = await saveBlob(blob, name, { title: 'Save the backup', extensions: ['cops'] });
      if (out.cancelled) { setSaving(false); return; }
      setSaved(true);
      // Only this settles the month — the file actually exists now.
      localStorage.setItem(DONE_KEY, monthKey());
      localStorage.removeItem(SNOOZE_KEY);
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
            onClick={notNow}
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
          Save the file and you will not be asked again this month. Choose
          &ldquo;Not now&rdquo; and it will ask again in a few days.
        </p>
      </div>
    </div>
  );
}
