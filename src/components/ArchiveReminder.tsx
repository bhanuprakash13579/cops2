import { useEffect, useState } from 'react';
import { useLocation } from 'react-router-dom';
import { AlertTriangle, X } from 'lucide-react';
import api from '@/lib/api';

/**
 * A quiet reminder to take a backup out of the building.
 *
 * WHY THIS IS NEEDED WHEN AUTOMATIC BACKUPS ALREADY RUN
 * Automatic backups go to folders on the office network. That protects against
 * one PC failing, which is the common case — and against nothing else. A fire, a
 * theft, or ransomware reaching the share takes every copy at once, because they
 * are all in the same building on the same network. The only protection against
 * that is a copy somebody carries away, and nothing else in the program asks for
 * one.
 *
 * WHY IT NEVER BLOCKS
 * It is tempting to make the app refuse to work until a backup is taken, and it
 * was considered. It turns a backup problem into an outage: the officers still
 * have passengers at the counter and cases to book, and a program that stops
 * them because of a file they cannot write is a worse failure than the one it is
 * preventing. So this is a bar that can be dismissed, every time.
 *
 * WHAT IT DELIBERATELY AVOIDS
 * It must never be a full-screen overlay. TrialBanner already owns that, and its
 * overlay carries an invisible four-click corner spot that is the administrator's
 * ONLY way in once a licence expires. A second overlay on top would cover it and
 * lock the administrator out of their own machine — nearly shipped exactly that
 * in the sibling project. This sits at the bottom, below TrialBanner's z-index,
 * and stays clear of the bottom-left corner.
 */
export default function ArchiveReminder() {
  const [state, setState] = useState<string | null>(null);
  const [days, setDays] = useState<number | null>(null);
  const [hidden, setHidden] = useState(false);
  const location = useLocation();

  useEffect(() => {
    let cancelled = false;
    // Deliberately NOT the shared api client. Its response interceptor treats
    // any 401 as the session ending — it clears cops_token and cops_user and
    // fires auth_declined. That is right for a call the officer made, and wrong
    // for a background poll: an expired or missing token would log somebody out
    // in the middle of typing a case, caused by a reminder they never asked for.
    // A plain fetch cannot do that.
    const check = async () => {
      const token = localStorage.getItem('cops_token');
      if (!token) return;   // nobody signed in; nothing to remind
      try {
        const res = await fetch(`${api.defaults.baseURL}/backup/archive/status`, {
          headers: { Authorization: `Bearer ${token}` },
        });
        if (!res.ok || cancelled) return;
        const data = await res.json();
        if (cancelled) return;
        setState(data.state);
        setDays(data.days_since ?? null);
      } catch {
        // Never surfaced. This is a nudge; if the check cannot run the officer
        // has nothing to act on, and a broken reminder must not look like a
        // broken app.
      }
    };
    // Not at startup — the first minute belongs to the officer getting to work.
    const first = setTimeout(check, 60_000);
    const repeat = setInterval(check, 6 * 60 * 60 * 1000);
    return () => { cancelled = true; clearTimeout(first); clearInterval(repeat); };
  }, []);

  // Nothing to say, dismissed for this session, or the officer is already on the
  // page that takes a backup — reminding them there would be nagging.
  if (hidden) return null;
  if (state !== 'due' && state !== 'overdue' && state !== 'never') return null;
  if (location.pathname.startsWith('/restore-backup')) return null;

  const overdue = state === 'overdue' || state === 'never';

  return (
    <div
      className={`fixed bottom-0 left-0 right-0 z-[9990] print:hidden
                  flex items-center gap-3 px-4 py-2 text-xs border-t
                  ${overdue
                    ? 'bg-red-50 border-red-200 text-red-800'
                    : 'bg-amber-50 border-amber-200 text-amber-900'}`}
      // Clear of the bottom-left corner, which TrialBanner's overlay uses as the
      // administrator's hidden way in.
      style={{ paddingLeft: '5.5rem' }}
      role="status"
    >
      <AlertTriangle size={14} className="shrink-0" />
      <span className="flex-1">
        {state === 'never'
          ? 'No backup has ever been saved off this network. The automatic backups all sit in this office — a fire or theft would take every copy at once.'
          : `The last backup taken out of the office was ${days} days ago. The automatic ones are all on this network, so they would be lost together.`}
        {' '}Admin → Backup → Save Backup, then keep the file somewhere else.
      </span>
      <button
        onClick={() => setHidden(true)}
        className="shrink-0 p-1 rounded hover:bg-black/5"
        aria-label="Dismiss until next time"
        title="Dismiss"
      >
        <X size={14} />
      </button>
    </div>
  );
}
