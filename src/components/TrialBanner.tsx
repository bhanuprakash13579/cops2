import { useState, useEffect } from 'react';
import { Clock, AlertTriangle, XCircle, ExternalLink, Mail } from 'lucide-react';
import { useNavigate, useLocation } from 'react-router-dom';
import { useTrialStatus } from '@/hooks/useTrialStatus';

// Tauri-friendly URL opener (same pattern as ModuleSelection.tsx)
async function openExternal(url: string) {
  try {
    const tauri = await import('@tauri-apps/plugin-shell');
    await tauri.open(url);
  } catch {
    window.open(url, '_blank', 'noopener,noreferrer');
  }
}

export default function TrialBanner() {
  const { trial_disabled, days_remaining, trial_days, expired, isLoading } = useTrialStatus();
  const navigate = useNavigate();
  const location = useLocation();

  // Hidden admin access — same secret gesture as the module-selection page:
  // 4 quick clicks (within 1.5 s) on an invisible corner spot open the admin panel.
  // No visible button is shown, so the admin route is never exposed on the
  // trial-expired block screen.
  const [secretClicks, setSecretClicks] = useState(0);
  useEffect(() => {
    if (!secretClicks) return;
    const t = setTimeout(() => setSecretClicks(0), 1500);
    return () => clearTimeout(t);
  }, [secretClicks]);
  const handleSecretClick = () => {
    setSecretClicks(c => {
      const next = c + 1;
      if (next >= 4) { navigate('/restore-backup'); return 0; }
      return next;
    });
  };

  if (isLoading || trial_disabled) return null;

  // Never show the trial overlay on the admin panel itself — admins must be
  // able to extend the trial / activate a license even after expiry.
  if (location.pathname.startsWith('/restore-backup')) return null;

  // ── Expired: full-screen block ─────────────────────────────────────────────
  // There is NO visible admin button — the only way in from this screen is the
  // hidden 4-click corner spot below, so a casual user cannot reach the admin
  // login just by reading the screen.
  if (expired) {
    return (
      <div className="fixed inset-0 z-[9999] flex items-center justify-center bg-slate-900/90 backdrop-blur-sm print:hidden p-4">
        {/* Hidden admin access — invisible 4-click corner spot (no visual clue).
            Mirrors the secret trigger on the module-selection screen. */}
        <div
          onClick={handleSecretClick}
          className="absolute bottom-6 left-0 w-16 h-16 z-[10000]"
          style={{ cursor: 'default' }}
        />

        <div className="bg-white rounded-2xl shadow-2xl p-8 max-w-md w-full text-center space-y-5">
          <XCircle size={56} className="text-red-500 mx-auto" />
          <h2 className="text-2xl font-bold text-slate-800">Trial Period Ended</h2>
          <p className="text-slate-600 text-sm leading-relaxed">
            The {trial_days}-day trial for this COPS installation has ended.
            Please contact us to renew or extend your license.
          </p>

          {/* Company contact */}
          <div className="bg-slate-50 rounded-xl border border-slate-200 p-4 space-y-2">
            <p className="text-[11px] font-semibold text-slate-500 uppercase tracking-wider">Contact</p>
            <button
              onClick={() => openExternal('https://www.gsicorp.in')}
              className="flex items-center justify-center gap-1.5 w-full text-sm text-blue-700 hover:text-blue-900 hover:underline"
            >
              <ExternalLink size={13} />
              www.gsicorp.in
            </button>
            <button
              onClick={() => openExternal('mailto:contact@gsicorp.in')}
              className="flex items-center justify-center gap-1.5 w-full text-sm text-blue-700 hover:text-blue-900 hover:underline"
            >
              <Mail size={13} />
              contact@gsicorp.in
            </button>
          </div>
        </div>
      </div>
    );
  }

  // ── Active: countdown banner ───────────────────────────────────────────────
  const d = days_remaining ?? trial_days;
  const color =
    d <= 3  ? 'bg-red-600 text-white'    :
    d <= 7  ? 'bg-orange-500 text-white' :
    d <= 14 ? 'bg-amber-400 text-amber-900' :
              'bg-blue-600 text-white';

  const Icon = d <= 7 ? AlertTriangle : Clock;

  return (
    <div
      className={`fixed top-0 left-0 right-0 z-[9998] flex items-center justify-center gap-2 text-xs font-semibold py-1 px-4 print:hidden ${color}`}
      style={{ height: '26px' }}
    >
      <Icon size={13} />
      <span>
        {d === 0
          ? 'Trial expires TODAY — contact admin'
          : `Trial: ${d} day${d === 1 ? '' : 's'} remaining`}
      </span>
    </div>
  );
}
