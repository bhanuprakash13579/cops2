import { useState, useEffect, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { ArrowLeft, ClipboardList, Sun, Moon, Calendar, CheckCircle, Download, X } from 'lucide-react';
import api from '@/lib/api';
import SessionSetup from './SessionSetup';
import RevenueSheet from './RevenueSheet';
import MessageGenerator from './MessageGenerator';
import FormulaRulesPage from './FormulaRulesPage';
import type { DrSession, DrFormulaRule } from './revenueCalc';
import { fmtDate } from './revenueCalc';
import { downloadAdcExcel, downloadRevenueExcel, downloadMonthlyRevenueExcel } from './dcrExcel';

type View = 'dashboard' | 'sheet' | 'new' | 'config';

export default function DcrModule() {
  const navigate = useNavigate();
  const [view, setView] = useState<View>('dashboard');
  const [session, setSession] = useState<DrSession | null>(null);
  const [sessions, setSessions] = useState<DrSession[]>([]);
  const [loadingSessions, setLoadingSessions] = useState(true);
  const [showMessage, setShowMessage] = useState(false);
  const [filterDate, setFilterDate] = useState('');
  // Month-end register. Defaults to the month being worked, which is the one an
  // officer wants nine times out of ten.
  const [reportMonth, setReportMonth] = useState<string>(
    () => new Date().toISOString().slice(0, 7),
  );
  const [monthBusy, setMonthBusy] = useState(false);

  const [rules, setRules] = useState<DrFormulaRule[]>([]);
  const [rulesLoaded, setRulesLoaded] = useState(false);

  const loadSessions = useCallback(() => {
    setLoadingSessions(true);
    api.get('/dcr/sessions')
      .then(r => setSessions(r.data))
      .catch(() => {})
      .finally(() => setLoadingSessions(false));
  }, []);

  useEffect(() => {
    loadSessions();
    api.get('/dcr/formula-rules')
      .then(r => { setRules(r.data.items ?? r.data); setRulesLoaded(true); })
      .catch(() => setRulesLoaded(true));
  }, []); // eslint-disable-line

  /** The rules as they stood on a given day — see loadRulesFor's note. */
  const loadRulesFor = async (reportDate?: string) => {
    // A formula changed since then applies from the day it was changed, so a
    // sheet reopened from last year still computes the way it did last year.
    try {
      const r = await api.get('/dcr/formula-rules',
                              reportDate ? { params: { as_of: reportDate } } : undefined);
      setRules(r.data.items ?? r.data);
    } catch { /* keep the rules already loaded */ }
  };

  const openSession = async (s: DrSession) => {
    try {
      const res = await api.get(`/dcr/sessions/${s.id}`);
      await loadRulesFor(s.report_date);
      setSession(res.data);
      setView('sheet');
    } catch { /* silent */ }
  };

  const handleSessionReady = (s: DrSession) => {
    setSession(s);
    setView('sheet');
    loadSessions();
  };

  const handleBack = () => {
    setSession(null);
    setView('dashboard');
    loadSessions();
  };

  const handleRulesChanged = useCallback((updated: DrFormulaRule[]) => {
    setRules(updated);
    setRulesLoaded(true);
  }, []);

  // ── Dashboard downloads: fetch full session data then generate Excel client-side ──

  const dashboardDownloadAdc = useCallback(async (s: DrSession, e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      const res = await api.get(`/dcr/sessions/${s.id}`);
      const full: DrSession = res.data;
      downloadAdcExcel(full, full.entries);
    } catch { /* silent */ }
  }, []);

  const dashboardDownloadRevenue = useCallback(async (s: DrSession, e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      // Fetch all sessions for the date (already have `s` as one of them)
      const listRes = await api.get('/dcr/sessions', { params: { date: s.report_date } });
      const sessionsForDate: DrSession[] = listRes.data;

      // Fetch full data for both shifts in parallel
      const [dayFull, nightFull] = await Promise.all(
        (['DAY', 'NIGHT'] as const).map(async (shift) => {
          const meta = sessionsForDate.find(x => x.shift === shift);
          if (!meta) return null;
          const r = await api.get(`/dcr/sessions/${meta.id}`);
          return r.data as DrSession;
        })
      );

      downloadRevenueExcel(s.report_date, dayFull, nightFull);
    } catch { /* silent */ }
  }, []);

  const downloadMonthly = async () => {
    const [y, m] = reportMonth.split('-').map(Number);
    if (!y || !m) return;
    setMonthBusy(true);
    try {
      // One request for the whole month. Session by session would be up to 62
      // round trips for one button press.
      const { data } = await api.get(`/dcr/month/${y}/${m}`, { timeout: 0 });
      const sessions = data.sessions ?? [];
      if (sessions.length === 0) {
        alert(`No duty sessions were recorded in ${reportMonth}.`);
        return;
      }
      await downloadMonthlyRevenueExcel(y, m, sessions);
    } catch (err: unknown) {
      const detail = (err as { response?: { data?: { detail?: string } } })?.response?.data?.detail;
      alert(`Could not build the monthly register: ${detail ?? (err instanceof Error ? err.message : String(err))}`);
    } finally {
      setMonthBusy(false);
    }
  };

  // ── Config page ──────────────────────────────────────────────────────────
  if (view === 'config') {

  return (
      <FormulaRulesPage
        onBack={() => setView('dashboard')}
        onRulesChanged={handleRulesChanged}
      />
    );
  }

  // ── New session ──────────────────────────────────────────────────────────
  if (view === 'new') {
    return (
      <div className="h-screen flex flex-col">
        <div className="flex items-center gap-3 px-4 py-3 bg-slate-800 text-white shrink-0">
          <button onClick={() => setView('dashboard')} className="text-slate-300 hover:text-white">
            <ArrowLeft size={18} />
          </button>
          <span className="font-semibold">New Revenue Session</span>
        </div>
        <SessionSetup onSessionReady={handleSessionReady} />
      </div>
    );
  }

  // ── Sheet view ───────────────────────────────────────────────────────────
  if (view === 'sheet' && session) {
    if (!rulesLoaded) {
      return (
        <div className="h-screen flex items-center justify-center bg-slate-900">
          <p className="text-blue-300 text-sm animate-pulse">Loading formula rules…</p>
        </div>
      );
    }
    return (
      <div className="h-screen flex flex-col">
        <div className="flex items-center gap-3 px-4 py-2 bg-slate-800 text-white shrink-0">
          <button onClick={handleBack} className="text-slate-300 hover:text-white flex items-center gap-1.5 text-sm">
            <ArrowLeft size={16} /> Sessions
          </button>
          <span className="text-slate-400">·</span>
          <span className="text-sm font-semibold">
            {fmtDate(session.report_date)} · {session.shift} · {session.batch_name}
          </span>
          {session.submitted_at && (
            <span className="ml-2 text-xs font-semibold text-emerald-400 flex items-center gap-1">
              <CheckCircle size={12} /> Submitted
            </span>
          )}
        </div>

        <div className="flex-1 overflow-hidden">
          <RevenueSheet
            session={session}
            rules={rules}
            onMessage={() => setShowMessage(true)}
            onConfig={() => setView('config')}
            onRulesChanged={() => loadRulesFor(session?.report_date)}
          />
        </div>

        {showMessage && session && (
          <MessageGenerator session={session} onClose={() => setShowMessage(false)} />
        )}
      </div>
    );
  }

  // ── Dashboard ────────────────────────────────────────────────────────────

  const dateMap = sessions.reduce<Record<string, DrSession[]>>((acc, s) => {
    if (!acc[s.report_date]) acc[s.report_date] = [];
    acc[s.report_date].push(s);
    return acc;
  }, {});

  const sortedDates = Object.keys(dateMap).sort((a, b) => b.localeCompare(a));
  const visibleDates = filterDate ? sortedDates.filter(d => d === filterDate) : sortedDates;

  return (
    <div className="min-h-screen bg-slate-50">
      <div className="flex items-center gap-3 px-4 py-3 bg-slate-800 text-white">
        <button onClick={() => navigate('/modules')} className="text-slate-300 hover:text-white">
          <ArrowLeft size={18} />
        </button>
        <ClipboardList size={18} className="text-teal-400" />
        <span className="font-bold text-lg">Revenue Report Module</span>
      </div>

      <div className="max-w-4xl mx-auto p-6 space-y-6">
        <div className="flex items-center justify-between gap-4 flex-wrap">
          <h2 className="text-lg font-bold text-slate-800">Sessions</h2>
          <div className="flex items-center gap-2 flex-wrap">
            {/* Month-end register — the figures the office files. */}
            <div className="flex items-center gap-1.5 bg-white border border-slate-300 rounded-xl px-3 py-1.5">
              <input
                type="month"
                value={reportMonth}
                onChange={e => setReportMonth(e.target.value)}
                className="text-sm text-slate-700 bg-transparent focus:outline-none w-32"
                title="Month for the revenue register"
              />
              <button
                onClick={downloadMonthly}
                disabled={monthBusy}
                className="text-xs font-semibold text-teal-700 hover:text-teal-900 disabled:opacity-50 whitespace-nowrap"
                title="Monthly Revenue Register — every session in the month"
              >
                {monthBusy ? 'Building…' : 'Monthly Register'}
              </button>
            </div>
            <div className="flex items-center gap-1.5 bg-white border border-slate-300 rounded-xl px-3 py-1.5">
              <Calendar size={14} className="text-slate-500 shrink-0" />
              <input
                type="date"
                value={filterDate}
                onChange={e => setFilterDate(e.target.value)}
                className="text-sm text-slate-700 bg-transparent focus:outline-none w-36"
                title="Filter by date — type or click to pick"
              />
              {filterDate && (
                <button onClick={() => setFilterDate('')} className="text-slate-400 hover:text-slate-600 ml-0.5" title="Clear">
                  <X size={13} />
                </button>
              )}
            </div>
            <button
              onClick={() => setView('config')}
              className="px-4 py-2 text-sm font-semibold bg-white border border-slate-300 text-slate-700 rounded-xl hover:bg-slate-50 flex items-center gap-1.5"
            >
              ⚙ Rates &amp; Formulas
            </button>
            <button
              onClick={() => setView('new')}
              className="px-4 py-2 text-sm font-semibold bg-teal-600 hover:bg-teal-700 text-white rounded-xl"
            >
              + New Session
            </button>
          </div>
        </div>

        {loadingSessions ? (
          <p className="text-sm text-slate-400">Loading sessions…</p>
        ) : sessions.length === 0 ? (
          <div className="text-center py-16 bg-white rounded-2xl border border-slate-200">
            <ClipboardList size={40} className="text-slate-300 mx-auto mb-3" />
            <p className="text-slate-500 font-medium">No sessions yet</p>
            <p className="text-slate-400 text-sm mt-1">Click "New Session" to start a duty collection report</p>
          </div>
        ) : visibleDates.length === 0 ? (
          <div className="text-center py-12 bg-white rounded-2xl border border-slate-200">
            <Calendar size={36} className="text-slate-300 mx-auto mb-3" />
            <p className="text-slate-500 font-medium">No sessions on {fmtDate(filterDate)}</p>
            <button onClick={() => setFilterDate('')} className="mt-2 text-sm text-teal-600 hover:underline">
              Clear filter to see all dates
            </button>
          </div>
        ) : (
          <div className="space-y-3">
            {visibleDates.map(date => {
              const daySessions = dateMap[date];
              const firstSession = daySessions[0];
              return (
                <div key={date} className="bg-white rounded-2xl border border-slate-200 overflow-hidden">
                  <div className="px-5 py-3 bg-slate-50 border-b border-slate-200 flex items-center gap-2">
                    <Calendar size={14} className="text-slate-500" />
                    <span className="text-sm font-bold text-slate-700 flex-1">{fmtDate(date)}</span>
                    <button
                      onClick={e => dashboardDownloadRevenue(firstSession, e)}
                      className="flex items-center gap-1 px-3 py-1 text-xs font-semibold bg-violet-600 hover:bg-violet-700 text-white rounded-lg"
                      title={`Revenue Report for ${fmtDate(date)} (both shifts)`}
                    >
                      <Download size={11} /> Revenue
                    </button>
                  </div>

                  <div className="divide-y divide-slate-100">
                    {daySessions.map(s => (
                      <div key={s.id} className="flex items-center gap-4 px-5 py-4 hover:bg-slate-50 transition-colors group">
                        <div className={`p-2 rounded-xl shrink-0 ${s.shift === 'DAY' ? 'bg-amber-100' : 'bg-indigo-100'}`}>
                          {s.shift === 'DAY'
                            ? <Sun size={18} className="text-amber-600" />
                            : <Moon size={18} className="text-indigo-600" />
                          }
                        </div>

                        <button className="flex-1 min-w-0 text-left" onClick={() => openSession(s)}>
                          <p className="text-sm font-semibold text-slate-800">{s.shift} Shift · {s.batch_name}</p>
                          <p className="text-xs text-slate-500 mt-0.5">
                            Created {s.created_at ? new Date(s.created_at).toLocaleString() : '—'}
                            {s.tariff && ` · Tariff: ${s.tariff.label ?? s.tariff.effective_from}`}
                          </p>
                          {s.submitted_at && (
                            <p className="text-xs text-emerald-600 mt-0.5 flex items-center gap-1">
                              <CheckCircle size={10} />
                              Submitted by {s.submitted_by} on {new Date(s.submitted_at).toLocaleString()}
                            </p>
                          )}
                        </button>

                        <div className="flex items-center gap-2 shrink-0">
                          <button
                            onClick={e => dashboardDownloadAdc(s, e)}
                            className="flex items-center gap-1 px-2.5 py-1 text-xs font-semibold bg-blue-600 hover:bg-blue-700 text-white rounded-lg"
                            title={`ADC Report — ${s.shift} shift`}
                          >
                            <Download size={11} /> ADC
                          </button>
                          {s.submitted_at
                            ? <span className="text-xs text-emerald-600 font-semibold bg-emerald-50 px-2 py-1 rounded-lg">✓ Submitted</span>
                            : <button onClick={() => openSession(s)} className="text-xs text-teal-600 font-semibold group-hover:underline px-2 py-1">Open →</button>
                          }
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
