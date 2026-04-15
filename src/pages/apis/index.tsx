import { useState, useRef, useCallback, useMemo, Component, ReactNode } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  ScanLine, LogOut, Upload, FileSpreadsheet, Download,
  CheckCircle2, AlertCircle, ChevronDown, ChevronRight,
  Shield, Fingerprint, Calendar, Plane, Users, FileSearch,
} from 'lucide-react';
import { useAuth } from '../../contexts/AuthContext';
import api from '../../lib/api';
import { showDownloadToast } from '@/components/DownloadToast';

// ── Types ─────────────────────────────────────────────────────────────────────
interface ItemRow {
  sno: number;
  desc: string;
  qty: number;
  uqc: string;
  value: number;
  duty: number;
}

interface CopsMatch {
  cops_id: number;
  cops_name: string;
  cops_passport: string;
  cops_dob: string;
  cops_nationality: string;
  match_type: 'PASSPORT' | 'DOB_NAME';
  match_score: number;
  os_no: string;
  os_year: number;
  os_date: string;
  location_code: string;
  total_items_value: number;
  total_duty_amount: number;
  total_payable: number;
  adjudication_date: string;
  adj_offr_name: string;
  items: ItemRow[];
}

interface PassengerResult {
  sno: number | null;
  apis_name: string;
  apis_passport: string;
  apis_dob: string;
  apis_flight: string;
  apis_sched_date: string;
  apis_gender: string;
  apis_nationality: string;
  apis_pnr: string;
  apis_route: string;
  case_count: number;
  cops_matches: CopsMatch[];
}

interface MatchResult {
  total_apis_passengers: number;
  matched_passengers: number;
  total_cases_found: number;
  results: PassengerResult[];
}

// ── Helpers ───────────────────────────────────────────────────────────────────

interface ParsedSubItem { name: string; qty: number | null; uqc: string | null; value: number | null; }

/**
 * Parse old VB6-migrated item descriptions like:
 *   "GOLD CHAIN x90.0 GMS = ₹394,938"
 *   "I PHONE 12 PRO X2.0 NOS = ₹1,60,000; AXE OIL x60.0 NOS = ₹1,200"
 * Returns null if the description does not match the pattern at all.
 */
function parseOldFormatDesc(desc: string): ParsedSubItem[] | null {
  if (!desc) return null;
  const RE = /^(.+?)\s+[xX]([\d,.]+)\s+(\w+)\s+=\s+[₹]?([\d,]+)$/;
  const parts = desc.split(/\s*;\s*/);
  let anyParsed = false;
  const result: ParsedSubItem[] = [];
  for (const part of parts) {
    const m = part.trim().match(RE);
    if (m) {
      anyParsed = true;
      result.push({
        name:  m[1].trim(),
        qty:   parseFloat(m[2].replace(/,/g, '')),
        uqc:   m[3].toUpperCase(),
        value: parseInt(m[4].replace(/,/g, ''), 10),
      });
    } else if (part.trim()) {
      result.push({ name: part.trim(), qty: null, uqc: null, value: null });
    }
  }
  return anyParsed ? result : null;
}

function fmt(n: number) {
  try {
    return n.toLocaleString('en-IN', { maximumFractionDigits: 0 });
  } catch {
    return Math.round(n).toLocaleString();
  }
}

function MatchBadge({ type }: { type: 'PASSPORT' | 'DOB_NAME' }) {
  return type === 'PASSPORT' ? (
    <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-semibold bg-emerald-100 text-emerald-800 border border-emerald-300">
      <Fingerprint size={11} /> Passport
    </span>
  ) : (
    <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-semibold bg-amber-100 text-amber-800 border border-amber-300">
      <Calendar size={11} /> DOB + Name
    </span>
  );
}

// ── Upload zone ───────────────────────────────────────────────────────────────
function UploadZone({
  onFile,
  loading,
}: {
  onFile: (f: File) => void;
  loading: boolean;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [dragging, setDragging] = useState(false);

  const handleDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      setDragging(false);
      const f = e.dataTransfer.files[0];
      if (f) onFile(f);
    },
    [onFile]
  );

  return (
    <div
      onDragOver={(e) => { e.preventDefault(); setDragging(true); }}
      onDragLeave={() => setDragging(false)}
      onDrop={handleDrop}
      onClick={() => !loading && inputRef.current?.click()}
      className={`
        relative flex flex-col items-center justify-center gap-4 p-12 rounded-2xl border-2 border-dashed
        transition-[border-color,background-color,transform,opacity] duration-200 cursor-pointer select-none
        ${dragging
          ? 'border-violet-400 bg-violet-50 scale-[1.01]'
          : 'border-slate-300 bg-white hover:border-violet-400 hover:bg-violet-50/40'
        }
        ${loading ? 'pointer-events-none opacity-60' : ''}
      `}
    >
      <input
        ref={inputRef}
        type="file"
        accept=".xlsx,.xls,.csv"
        className="hidden"
        onChange={(e) => e.target.files?.[0] && onFile(e.target.files[0])}
      />

      <div className="p-5 rounded-2xl bg-violet-100 border border-violet-200">
        {loading ? (
          <div className="w-12 h-12 border-4 border-violet-400 border-t-transparent rounded-full animate-spin" />
        ) : (
          <FileSpreadsheet className="w-12 h-12 text-violet-500" />
        )}
      </div>

      <div className="text-center">
        <p className="text-lg font-semibold text-slate-700">
          {loading ? 'Scanning passengers…' : 'Drop APIS Excel file here'}
        </p>
        <p className="text-sm text-slate-500 mt-1">
          {loading
            ? 'Matching against COPS database, please wait'
            : 'or click to browse — accepts .xlsx / .xls / .csv'}
        </p>
      </div>

      {!loading && (
        <button
          type="button"
          className="flex items-center gap-2 px-5 py-2.5 bg-violet-600 hover:bg-violet-700 text-white text-sm font-semibold rounded-xl transition-colors"
        >
          <Upload size={16} /> Select File
        </button>
      )}
    </div>
  );
}

// ── Case card ─────────────────────────────────────────────────────────────────
function CaseCard({ match, apisName }: { match: CopsMatch; apisName: string }) {
  const [open, setOpen] = useState(false);

  return (
    <div className={`rounded-xl border ${match.match_type === 'PASSPORT' ? 'border-emerald-200 bg-emerald-50/60' : 'border-amber-200 bg-amber-50/60'}`}>
      {/* Header row */}
      <button
        type="button"
        onClick={() => setOpen(v => !v)}
        className="w-full flex items-start gap-3 p-3 text-left"
      >
        <span className="mt-0.5 shrink-0 text-slate-400">
          {open ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
        </span>

        <div className="flex-1 min-w-0">
          <div className="flex flex-wrap items-center gap-2 mb-1">
            <span className="font-bold text-slate-800 text-sm">{match.os_no}/{match.os_year}</span>
            <MatchBadge type={match.match_type} />
            {match.adjudication_date && (
              <span className="text-xs text-slate-500">Adjudicated: {match.adjudication_date}</span>
            )}
          </div>

          {/* Name comparison — the gem the user asked for */}
          <div className="flex flex-wrap gap-4 text-xs mt-1">
            <div>
              <span className="text-slate-400 uppercase tracking-wide">APIS name · </span>
              <span className="font-medium text-slate-700">{apisName || '—'}</span>
            </div>
            <div>
              <span className="text-slate-400 uppercase tracking-wide">COPS name · </span>
              <span className={`font-medium ${apisName.toUpperCase() === match.cops_name.toUpperCase() ? 'text-emerald-700' : 'text-amber-700'}`}>
                {match.cops_name || '—'}
              </span>
            </div>
          </div>
        </div>

        <div className="text-right shrink-0 text-xs text-slate-600 space-y-0.5">
          <div>₹{fmt(match.total_payable)} payable</div>
          <div className="text-slate-400">{match.items.length} item{match.items.length !== 1 ? 's' : ''}</div>
        </div>
      </button>

      {/* Expanded detail */}
      {open && (
        <div className="px-4 pb-4 space-y-3 border-t border-inherit">
          {/* Passport / DOB side-by-side */}
          <div className="mt-3 grid grid-cols-2 gap-3 text-xs">
            <div className="bg-white/70 rounded-lg p-2.5 border border-inherit">
              <div className="text-slate-400 uppercase tracking-wide mb-1">Passport in APIS</div>
              <div className="font-mono font-semibold text-slate-700">{match.cops_passport || 'Same as APIS'}</div>
            </div>
            <div className="bg-white/70 rounded-lg p-2.5 border border-inherit">
              <div className="text-slate-400 uppercase tracking-wide mb-1">DOB in COPS</div>
              <div className="font-mono font-semibold text-slate-700">{match.cops_dob || '—'}</div>
            </div>
          </div>

          {/* Financials */}
          <div className="grid grid-cols-3 gap-2 text-xs">
            {[
              ['Items Value', match.total_items_value],
              ['Duty Amount', match.total_duty_amount],
              ['Total Payable', match.total_payable],
            ].map(([label, val]) => (
              <div key={label as string} className="bg-white/70 rounded-lg p-2.5 border border-inherit text-center">
                <div className="text-slate-400">{label as string}</div>
                <div className="font-semibold text-slate-800 text-sm">₹{fmt(val as number)}</div>
              </div>
            ))}
          </div>

          {/* Items — smart display: old records have no per-item value/duty */}
          {match.items.length > 0 && (() => {
            const hasValues = match.items.some(it => it.value > 0);
            const hasDuty   = match.items.some(it => it.duty  > 0);
            const hasQty    = match.items.some(it => it.qty   > 0);

            if (!hasValues) {
              // Legacy / old records — DB value columns are 0.
              // The description field may contain old VB6 formatted text like
              // "GOLD CHAIN x90.0 GMS = ₹394,938; AXE OIL x60.0 NOS = ₹1,200"
              // Try to parse each description and build a clean structured display.
              const allParsed: { sno: number; sub: ParsedSubItem }[] = [];
              let allCouldParse = true;
              for (const it of match.items) {
                const parsed = parseOldFormatDesc(it.desc);
                if (parsed) {
                  parsed.forEach(sub => allParsed.push({ sno: it.sno, sub }));
                } else {
                  allCouldParse = false;
                  allParsed.push({ sno: it.sno, sub: { name: it.desc, qty: null, uqc: null, value: null } });
                }
              }
              const parsedHasValues = allParsed.some(r => (r.sub.value ?? 0) > 0);

              return (
                <div className="rounded-lg border border-inherit overflow-hidden">
                  <div className="bg-white/50 px-3 py-1.5 flex items-center justify-between border-b border-inherit">
                    <span className="text-xs font-medium text-slate-500">Seized Items</span>
                    <span className="text-xs text-slate-400 italic">
                      {allCouldParse ? 'Values extracted from legacy record description' : 'Legacy record'}
                    </span>
                  </div>
                  <table className="w-full text-xs">
                    <thead>
                      <tr className="bg-white/30">
                        <th className="px-2 py-1.5 text-left text-slate-400 font-medium w-6">#</th>
                        <th className="px-2 py-1.5 text-left text-slate-400 font-medium">Description</th>
                        <th className="px-2 py-1.5 text-right text-slate-400 font-medium">Qty</th>
                        {parsedHasValues && <th className="px-2 py-1.5 text-right text-slate-400 font-medium">Value (₹)</th>}
                      </tr>
                    </thead>
                    <tbody>
                      {allParsed.map((r, i) => (
                        <tr key={i} className="border-t border-inherit even:bg-white/30">
                          <td className="px-2 py-1.5 text-slate-400">{r.sno}</td>
                          <td className="px-2 py-1.5 text-slate-700">{r.sub.name}</td>
                          <td className="px-2 py-1.5 text-right text-slate-500 font-mono">
                            {r.sub.qty != null ? `${r.sub.qty} ${r.sub.uqc}` : '—'}
                          </td>
                          {parsedHasValues && (
                            <td className="px-2 py-1.5 text-right text-slate-700">
                              {r.sub.value != null && r.sub.value > 0 ? fmt(r.sub.value) : '—'}
                            </td>
                          )}
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              );
            }

            // New-format records — individual values present; show structured table.
            return (
              <div className="overflow-auto rounded-lg border border-inherit">
                <table className="w-full text-xs">
                  <thead>
                    <tr className="bg-white/50">
                      <th className="px-2 py-1.5 text-left text-slate-500 font-medium">#</th>
                      <th className="px-2 py-1.5 text-left text-slate-500 font-medium">Description</th>
                      {hasQty && <th className="px-2 py-1.5 text-right text-slate-500 font-medium">Qty</th>}
                      <th className="px-2 py-1.5 text-right text-slate-500 font-medium">Value (₹)</th>
                      {hasDuty && <th className="px-2 py-1.5 text-right text-slate-500 font-medium">Duty (₹)</th>}
                    </tr>
                  </thead>
                  <tbody>
                    {match.items.map((it, i) => (
                      <tr key={i} className="border-t border-inherit even:bg-white/30">
                        <td className="px-2 py-1.5 text-slate-500">{it.sno}</td>
                        <td className="px-2 py-1.5 text-slate-700 font-medium">{it.desc}</td>
                        {hasQty && <td className="px-2 py-1.5 text-right text-slate-600">{it.qty} {it.uqc}</td>}
                        <td className="px-2 py-1.5 text-right text-slate-700">{fmt(it.value)}</td>
                        {hasDuty && <td className="px-2 py-1.5 text-right text-slate-700">{fmt(it.duty)}</td>}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            );
          })()}

          {match.adj_offr_name && (
            <p className="text-xs text-slate-500">
              Adjudicating Officer: <span className="font-medium text-slate-700">{match.adj_offr_name}</span>
            </p>
          )}
        </div>
      )}
    </div>
  );
}

// ── Passenger row ─────────────────────────────────────────────────────────────
function PassengerRow({ pax }: { pax: PassengerResult }) {
  const [open, setOpen] = useState(false);

  return (
    <div className="border border-slate-200 rounded-xl bg-white shadow-sm overflow-hidden">
      {/* Summary bar */}
      <button
        type="button"
        onClick={() => setOpen(v => !v)}
        className="w-full flex items-center gap-3 px-4 py-3 text-left hover:bg-slate-50 transition-colors"
      >
        <span className="text-slate-400 shrink-0">
          {open ? <ChevronDown size={18} /> : <ChevronRight size={18} />}
        </span>

        {/* Name + passport */}
        <div className="flex-1 min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-bold text-slate-800">{pax.apis_name}</span>
            <span className="text-xs text-slate-400 font-mono">{pax.apis_passport}</span>
            <span className="text-xs text-slate-400">{pax.apis_dob}</span>
            {pax.apis_gender && (
              <span className="text-xs text-slate-400">{pax.apis_gender}</span>
            )}
          </div>
          <div className="flex flex-wrap gap-3 mt-0.5 text-xs text-slate-500">
            {pax.apis_flight && (
              <span className="flex items-center gap-1"><Plane size={11} /> {pax.apis_flight}</span>
            )}
            {pax.apis_sched_date && <span>{pax.apis_sched_date}</span>}
            {pax.apis_route && <span>{pax.apis_route}</span>}
          </div>
        </div>

        {/* Case count badge */}
        <div className="shrink-0 flex items-center gap-1.5">
          <span className={`inline-flex items-center gap-1 px-3 py-1 rounded-full text-sm font-bold
            ${pax.case_count === 1
              ? 'bg-orange-100 text-orange-700 border border-orange-300'
              : 'bg-red-100 text-red-700 border border-red-300'
            }`}>
            <Shield size={13} />
            {pax.case_count} case{pax.case_count !== 1 ? 's' : ''}
          </span>
        </div>
      </button>

      {/* Expanded cases */}
      {open && (
        <div className="px-4 pb-4 space-y-2 border-t border-slate-100 pt-3">
          {pax.cops_matches.map((match, i) => (
            <CaseCard key={i} match={match} apisName={pax.apis_name} />
          ))}
        </div>
      )}
    </div>
  );
}

// ── Error Boundary ────────────────────────────────────────────────────────────
class ApisErrorBoundary extends Component<{ children: ReactNode }, { error: string | null }> {
  state = { error: null };
  static getDerivedStateFromError(e: Error) { return { error: e.message }; }
  render() {
    if (this.state.error) return (
      <div className="min-h-screen bg-slate-50 flex items-center justify-center p-8">
        <div className="bg-red-50 border border-red-200 rounded-xl p-6 max-w-lg w-full">
          <p className="font-bold text-red-700 mb-2">Render error — please report this</p>
          <pre className="text-xs text-red-600 whitespace-pre-wrap">{this.state.error}</pre>
          <button onClick={() => this.setState({ error: null })}
            className="mt-4 px-4 py-2 bg-red-600 text-white rounded-lg text-sm font-semibold">
            Retry
          </button>
        </div>
      </div>
    );
    return this.props.children;
  }
}

// ── Main module ───────────────────────────────────────────────────────────────
function ApisModuleInner() {
  const { user, token, logout } = useAuth();
  const navigate = useNavigate();

  const [loading, setLoading]   = useState(false);
  const [result, setResult]     = useState<MatchResult | null>(null);
  const [error, setError]       = useState<string | null>(null);
  const [currentFile, setCurrentFile] = useState<File | null>(null);
  const [exporting, setExporting] = useState(false);
  const [filterText, setFilterText] = useState('');

  const handleLogout = () => { logout(); navigate('/modules'); };




  const runMatch = useCallback(async (file: File) => {
    setLoading(true);
    setError(null);
    setCurrentFile(file);
    setFilterText('');

    try {
      const fd = new FormData();
      fd.append('file', file);

      const res = await api.post<MatchResult>('/apis/match', fd, {
        headers: { Authorization: `Bearer ${token}`, 'Content-Type': undefined },
      });
      setResult(res.data);
    } catch (e: any) {
      const detail = e.response?.data?.detail;
      const msg = Array.isArray(detail)
        ? detail.map((d: any) => d.msg || JSON.stringify(d)).join('; ')
        : (typeof detail === 'string' ? detail : e.message || 'Unknown error');
      setError(msg);
    } finally {
      setLoading(false);
    }
  }, [token]);

  const handleExport = useCallback(async () => {
    if (!currentFile) return;
    setExporting(true);
    try {
      const fd = new FormData();
      fd.append('file', currentFile);

      const res = await api.post('/apis/export', fd, {
        headers: { Authorization: `Bearer ${token}`, 'Content-Type': undefined },
        responseType: 'blob',
      });

      const cd   = res.headers['content-disposition'] || '';
      const name = cd.match(/filename="([^"]+)"/)?.[1] || 'COPS_APIS_Match.xlsx';

      try {
        const { save } = await import('@tauri-apps/plugin-dialog');
        const { writeFile } = await import('@tauri-apps/plugin-fs');
        const savePath = await save({ title: 'Save Report', defaultPath: name, filters: [{ name: 'Excel', extensions: ['xlsx'] }] });
        if (savePath) {
          const arrayBuf = await (res.data as Blob).arrayBuffer();
          await writeFile(savePath, new Uint8Array(arrayBuf));
          showDownloadToast(`Report saved to ${savePath}`);
        }
      } catch {
        const url  = URL.createObjectURL(res.data);
        const a    = document.createElement('a');
        a.href = url; a.download = name; a.click();
        URL.revokeObjectURL(url);
        showDownloadToast(`Report downloaded as ${name}`);
      }
    } catch (e: any) {
      setError(e.response?.data?.detail || e.message || 'Export failed');
    } finally {
      setExporting(false);
    }
  }, [currentFile, token]);

  // Filtered results — memoized so the filter only re-runs when result or filterText changes
  const filtered = useMemo(() => {
    if (!result) return [];
    if (!filterText.trim()) return result.results;
    const needle = filterText.toLowerCase();
    return result.results.filter(p =>
      p.apis_name.toLowerCase().includes(needle) ||
      p.apis_passport.toLowerCase().includes(needle) ||
      p.cops_matches.some(m =>
        m.cops_name.toLowerCase().includes(needle) ||
        m.os_no.toLowerCase().includes(needle)
      )
    );
  }, [result, filterText]);

  return (
    <div className="min-h-screen bg-slate-50 flex flex-col">

      {/* ── Top bar ─────────────────────────────────────────────────────── */}
      <header className="bg-slate-900 border-b border-slate-700 px-6 py-3 flex items-center gap-4 shrink-0">
        <div className="bg-violet-600 p-2 rounded-lg">
          <ScanLine className="text-white w-5 h-5" />
        </div>
        <div className="flex-1 min-w-0">
          <h1 className="text-white font-bold text-base leading-tight">COPS ↔ APIS</h1>
          <p className="text-violet-400 text-xs">Passenger Intelligence · {user?.user_name}</p>
        </div>
        <button
          type="button"
          onClick={handleLogout}
          className="flex items-center gap-1.5 text-slate-400 hover:text-white text-sm px-3 py-1.5 rounded-lg hover:bg-slate-800 transition-colors"
        >
          <LogOut size={15} /> Logout
        </button>
      </header>

      {/* ── Body ────────────────────────────────────────────────────────── */}
      <main className="flex-1 overflow-y-auto p-6 max-w-5xl mx-auto w-full">

        {/* Upload zone (always visible so user can scan another file) */}
        <div className="mb-6">
          <UploadZone onFile={runMatch} loading={loading} />
        </div>

        {/* Error */}
        {error && (
          <div className="mb-6 flex items-start gap-3 bg-red-50 border border-red-200 text-red-700 rounded-xl p-4">
            <AlertCircle size={18} className="shrink-0 mt-0.5" />
            <div>
              <p className="font-semibold">Match failed</p>
              <p className="text-sm mt-0.5">{error}</p>
            </div>
          </div>
        )}

        {/* Results */}
        {result && (
          <>
            {/* Summary stats */}
            <div className="grid grid-cols-3 gap-4 mb-6">
              {[
                { icon: Users,       label: 'Passengers scanned', value: result.total_apis_passengers, color: 'text-slate-700' },
                { icon: CheckCircle2, label: 'Matched in COPS',   value: result.matched_passengers,   color: 'text-violet-700' },
                { icon: FileSearch,  label: 'Total cases found',  value: result.total_cases_found,    color: 'text-red-600'    },
              ].map(({ icon: Icon, label, value, color }) => (
                <div key={label} className="bg-white rounded-xl border border-slate-200 p-4 flex items-center gap-4 shadow-sm">
                  <div className="bg-slate-100 p-2.5 rounded-lg shrink-0">
                    <Icon size={20} className={color} />
                  </div>
                  <div>
                    <p className={`text-2xl font-extrabold ${color}`}>{value}</p>
                    <p className="text-xs text-slate-500 mt-0.5">{label}</p>
                  </div>
                </div>
              ))}
            </div>

            {/* Actions bar */}
            <div className="flex flex-wrap items-center gap-3 mb-4">
              <input
                type="text"
                placeholder="Filter by name, passport, OS no…"
                value={filterText}
                onChange={e => setFilterText(e.target.value)}
                className="flex-1 min-w-48 px-3 py-2 text-sm border border-slate-200 rounded-lg focus:outline-none focus:ring-2 focus:ring-violet-400 bg-white"
              />
              <button
                type="button"
                onClick={handleExport}
                disabled={exporting}
                className="flex items-center gap-2 px-4 py-2 bg-violet-600 hover:bg-violet-700 disabled:opacity-60 text-white text-sm font-semibold rounded-lg transition-colors"
              >
                {exporting ? (
                  <span className="w-4 h-4 border-2 border-white border-t-transparent rounded-full animate-spin" />
                ) : (
                  <Download size={15} />
                )}
                Download Report
              </button>
            </div>



            {/* Legend */}
            <div className="flex flex-wrap gap-4 mb-4 text-xs text-slate-500">
              <span className="flex items-center gap-1.5">
                <span className="w-3 h-3 rounded-full bg-emerald-400 inline-block" />
                Passport match — highest confidence
              </span>
              <span className="flex items-center gap-1.5">
                <span className="w-3 h-3 rounded-full bg-amber-400 inline-block" />
                DOB + Name match — possible changed passport
              </span>
            </div>

            {/* Passenger list */}
            {filtered.length === 0 ? (
              <div className="text-center py-16 text-slate-400">
                No matches found{filterText ? ' for that filter' : ''}.
              </div>
            ) : (
              <div className="space-y-3">
                {filtered.map((pax, i) => (
                  <PassengerRow key={i} pax={pax} />
                ))}
              </div>
            )}
          </>
        )}
      </main>
    </div>
  );
}

export default function ApisModule() {
  return (
    <ApisErrorBoundary>
      <ApisModuleInner />
    </ApisErrorBoundary>
  );
}
