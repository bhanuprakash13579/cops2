import { useEffect, useState } from 'react';
import { useNavigate, useLocation } from 'react-router-dom';
import { Home, ChevronLeft } from 'lucide-react';

/**
 * A way out of every page, that no page has to remember to provide.
 *
 * The app runs in a desktop window with no browser chrome — no back arrow, no
 * address bar — so a page that forgets its own back button, or a screen that
 * errors half-drawn, leaves the officer with nowhere to go but to kill the app.
 * Each screen does carry its own navigation, but that is a promise thirty pages
 * have to keep, and the next page added is one more chance to break it.
 *
 * This does not depend on any of them. It sits at the app's root, above every
 * page, and gives two things that always work:
 *
 *   • a small control, bottom-left, out of the way of the work — Back, and Home
 *     to the module list;
 *   • the keyboard chords a desktop user reaches for by reflex —
 *     Alt+Left to go back, Alt+Home to the modules.
 *
 * It shows itself only where being stuck is possible: not on the module list
 * (which is home) and not on the sign-in or licence screens (where leaving is
 * the wrong thing and there is nowhere behind them to go). It hides itself for
 * printing, and sits below dialogs so it never swallows a click meant for one.
 */
export default function EscapeHatch() {
  const navigate = useNavigate();
  const location = useLocation();
  const [expanded, setExpanded] = useState(false);

  // Screens where an escape hatch is wrong, not merely unnecessary: home
  // itself, and the gates in front of it.
  const path = location.pathname;
  const suppressed =
    path === '/' ||
    path === '/modules' ||
    path.startsWith('/login') ||
    path.startsWith('/restore-backup'); // its own header carries Back to Modules

  useEffect(() => {
    if (suppressed) return;
    const onKey = (e: KeyboardEvent) => {
      // Alt chords — the browser's own Back/Home, which this window lacks.
      // Kept off the bare keys so they never fight a form or a shortcut.
      if (e.altKey && e.key === 'ArrowLeft') {
        e.preventDefault();
        // Prefer real history; fall back to the module list if there is none.
        if (window.history.length > 1) navigate(-1);
        else navigate('/modules');
      } else if (e.altKey && (e.key === 'Home' || e.key === 'h' || e.key === 'H')) {
        e.preventDefault();
        navigate('/modules');
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [suppressed, navigate]);

  if (suppressed) return null;

  return (
    <div
      className="fixed bottom-3 left-3 z-[9000] flex items-center gap-1 print:hidden
                 rounded-full bg-slate-800/90 text-white shadow-lg backdrop-blur-sm
                 border border-slate-700"
      onMouseEnter={() => setExpanded(true)}
      onMouseLeave={() => setExpanded(false)}
      role="navigation"
      aria-label="Escape to a safe screen"
    >
      <button
        type="button"
        onClick={() => (window.history.length > 1 ? navigate(-1) : navigate('/modules'))}
        title="Back (Alt + ←)"
        className="flex items-center gap-1 pl-3 pr-2 py-1.5 text-xs font-medium
                   hover:text-blue-300 rounded-l-full"
      >
        <ChevronLeft size={15} /> {expanded && <span>Back</span>}
      </button>
      <span className="w-px h-4 bg-slate-600" />
      <button
        type="button"
        onClick={() => navigate('/modules')}
        title="Module list (Alt + Home)"
        className="flex items-center gap-1 pl-2 pr-3 py-1.5 text-xs font-medium
                   hover:text-blue-300 rounded-r-full"
      >
        <Home size={14} /> {expanded && <span>Modules</span>}
      </button>
    </div>
  );
}
