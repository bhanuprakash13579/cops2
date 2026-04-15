import { useState, useEffect, useMemo, memo } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { Gavel, ArrowLeft, Save, XCircle, User, Package, FileText, AlertCircle, AlertTriangle, CheckCircle, Edit, Printer, Wand2, Trash2, Clock } from 'lucide-react';
import DatePicker from '@/components/DatePicker';
import api from '@/lib/api';
import { useAuth } from '@/contexts/AuthContext';
import { showDownloadToast } from '@/components/DownloadToast';
import { useRemarksGenerator, detectContextualQuestions, ContextualAnswers, ContextualQuestion } from '@/hooks/useRemarksGenerator';

// ── Types ─────────────────────────────────────────────────────────────────────

interface CopsItem {
  id: number;
  items_sno: number;
  items_desc?: string;
  items_qty: number;
  items_uqc?: string;
  items_value: number;
  value_per_piece?: number;
  items_release_category?: string;
  items_duty_type?: string;
  items_fa?: number;
  items_fa_type?: string;
  items_fa_qty?: number;
  items_fa_uqc?: string;
  items_duty?: number;
  cumulative_duty_rate?: number;
}

interface OSCase {
  id: number;
  os_no: string;
  os_date: string;
  os_year: number;
  pax_name?: string;
  pax_nationality?: string;
  passport_no?: string;
  father_name?: string;
  pax_date_of_birth?: string;
  pax_address1?: string;
  pax_address2?: string;
  pax_address3?: string;
  pax_status?: string;
  flight_no?: string;
  flight_date?: string;
  country_of_departure?: string;
  port_of_dep_dest?: string;
  arrived_from?: string;
  date_of_departure?: string;
  stay_abroad_days?: number | string;
  pp_issue_place?: string;
  residence_at?: string;
  booked_by?: string;
  detained_by?: string;
  detention_date?: string;
  case_type?: string;
  seal_no?: string;
  dr_no?: string;
  previous_os_details?: string;
  supdts_remarks?: string;
  total_items_value: number;
  total_items: number;
  items: CopsItem[];
  adjudication_date?: string;
  adj_offr_name?: string;
  adj_offr_designation?: string;
  adjn_offr_remarks?: string;
  adjn_section_ref?: string;
  adjudication_time?: string;
  online_adjn?: string;
  closure_ind?: string;
  rf_amount?: number;
  pp_amount?: number;
  ref_amount?: number;
  br_amount?: number;
  confiscated_value?: number;
  redeemed_value?: number;
  re_export_value?: number;
  total_duty_amount?: number;
}

// ── Pure helpers (module-level, never recreated) ──────────────────────────────

const _UQC_LABEL: Record<string, string> = {
  NOS: 'Nos.', STK: 'Sticks', KGS: 'Kgs.', GMS: 'Gms.', LTR: 'Ltrs.', MTR: 'Mtrs.', PRS: 'Pairs',
};
const _uqcLabel = (code: string) => _UQC_LABEL[(code || '').toUpperCase()] ?? (code || 'Nos.');
const _fmtQty = (q: number | string) => { const n = Number(q); return n % 1 === 0 ? Math.trunc(n).toString() : String(q); };

const fmtDateStr = (d: string | null | undefined): string => {
  if (!d) return '—';
  if (d === 'N.A.' || d === 'NA' || d === 'n.a.') return d;
  const parts = d.split('-');
  if (parts.length === 3 && parts[0].length === 4) return `${parts[2]}-${parts[1]}-${parts[0]}`;
  return d;
};

const fmtDate = (d: string | null | undefined): string => {
  if (!d) return '—';
  try { return new Date(d).toLocaleDateString('en-GB'); } catch { return d; }
};

const effFaRupees = (item: CopsItem): number => {
  const rc = item.items_release_category || '';
  if (!['Under Duty', 'Under OS', 'RF', 'REF'].includes(rc)) return 0;
  if ((item.items_fa_type || 'value') === 'qty') {
    const totalQty = item.items_qty || 0;
    const faQty = item.items_fa_qty || 0;
    return totalQty > 0 ? Math.min((faQty / totalQty) * (item.items_value || 0), item.items_value || 0) : 0;
  }
  return item.items_fa || 0;
};

const valueAfterFA = (item: CopsItem): number => {
  const totalVal = item.items_value || 0;
  const rc = item.items_release_category || '';
  if (rc === 'CONFS' || !rc) return totalVal;
  const faT = item.items_fa_type || 'value';
  if (faT === 'qty') {
    const tq = item.items_qty || 0;
    const fq = item.items_fa_qty || 0;
    const vpp = item.value_per_piece || 0;
    return Math.max(0, tq - fq) * vpp;
  }
  const fa = Math.min(item.items_fa || 0, totalVal);
  return Math.max(0, totalVal - fa);
};

const catLabel = (rc: string | undefined): { label: string; cls: string } => {
  switch (rc) {
    case 'Under Duty': return { label: 'Under Duty', cls: 'bg-yellow-100 border-yellow-300 text-yellow-800' };
    case 'CONFS':      return { label: 'Abs. Conf.', cls: 'bg-red-100 border-red-300 text-red-800' };
    case 'RF':         return { label: 'Redemption', cls: 'bg-green-100 border-green-300 text-green-800' };
    case 'REF':        return { label: 'Re-Export', cls: 'bg-blue-100 border-blue-300 text-blue-800' };
    default:           return { label: 'Under OS', cls: 'bg-slate-100 border-slate-300 text-slate-800' };
  }
};



// ── AdjItemRow ────────────────────────────────────────────────────────────────
const AdjItemRow = memo(function AdjItemRow({ item }: { item: CopsItem; }) {
  const displayCat = item.items_release_category || 'Under OS';
  const isConfs = displayCat === 'CONFS';
  const { label, cls } = catLabel(displayCat);

  const faRupees = effFaRupees(item);
  const faType = item.items_fa_type || 'value';
  const faDisplay = (displayCat !== 'Under Duty' && displayCat !== 'Under OS' && displayCat !== 'RF' && displayCat !== 'REF') ? '—'
    : faType === 'qty'
      ? (item.items_fa_qty ? `${item.items_fa_qty} ${item.items_fa_uqc || ''}`.trim() : '—')
      : (faRupees > 0 ? `₹${faRupees.toLocaleString('en-IN', { minimumFractionDigits: 2 })}` : '—');

  const rowBg = displayCat === 'CONFS' ? 'bg-red-50' : displayCat === 'Under Duty' ? 'bg-yellow-50'
    : displayCat === 'RF' ? 'bg-green-50/40' : displayCat === 'REF' ? 'bg-blue-50/40' : '';

  return (
    <tr className={`hover:bg-slate-50 ${rowBg}`}>
      <td className="px-4 py-2.5 text-center text-slate-500">{item.items_sno}</td>
      <td className="px-4 py-2.5 text-slate-800 font-medium">{item.items_desc || '—'}</td>
      <td className="px-4 py-2.5 text-slate-700 text-xs">{item.items_duty_type || '—'}</td>
      <td className="px-4 py-2.5 text-center text-slate-600">{item.items_qty} {item.items_uqc}</td>
      <td className="px-4 py-2.5 text-right text-slate-600 text-xs">
        ₹{(item.value_per_piece || 0).toLocaleString('en-IN', { minimumFractionDigits: 2 })}
      </td>
      <td className="px-4 py-2.5 text-right text-slate-500 text-xs">{faDisplay}</td>
      <td className="px-4 py-2.5 text-right font-bold text-amber-900 bg-amber-50">
        ₹{valueAfterFA(item).toLocaleString('en-IN', { minimumFractionDigits: 2 })}
      </td>
      <td className="px-4 py-2.5 text-center text-slate-500 text-xs">
        {isConfs ? <span className="text-red-400 italic">N/A</span> : `${item.cumulative_duty_rate ?? 0}%`}
      </td>
      <td className={`px-4 py-2.5 text-right font-bold ${isConfs ? 'text-red-400 italic' : 'text-slate-800'}`}>
        {isConfs ? 'N/A' : `₹${(item.items_duty || 0).toLocaleString('en-IN', { minimumFractionDigits: 2 })}`}
      </td>
      <td className="px-4 py-2.5 text-center">
        <span className={`inline-block px-2 py-1.5 border text-xs font-bold rounded-lg ${cls}`}>{label}</span>
      </td>
    </tr>
  );
});

// ── CaseDetailsPanel: memo — NEVER re-renders when adjData changes ─────────────
const CaseDetailsPanel = memo(function CaseDetailsPanel({ osCase }: { osCase: OSCase }) {
  const fields: [string, string | undefined][] = [
    ['Passenger Name', osCase.pax_name],
    ['Passport No.', osCase.passport_no],
    ['Place of Issue of PP', osCase.pp_issue_place],
    ['Nationality', osCase.pax_nationality],
    ['Normal Residence At', osCase.residence_at],
    ['Date of Birth', fmtDate(osCase.pax_date_of_birth)],
    ["Father's Name", osCase.father_name],
    ['Pax Status', osCase.pax_status],
    ['Case Type', osCase.case_type],
    ['Flight No.', osCase.flight_no],
    ['Flight Date', fmtDate(osCase.flight_date)],
    [osCase.case_type === 'Export Case' ? 'Proposed Date of Travel' : 'Date of Departure from India', fmtDateStr(osCase.date_of_departure)],
    [osCase.case_type === 'Export Case' ? 'Stay Abroad' : 'Stay Abroad (Days)', osCase.case_type === 'Export Case' ? 'N/A' : (osCase.stay_abroad_days != null ? String(osCase.stay_abroad_days) : undefined)],
    [osCase.case_type === 'Export Case' ? 'Supposed Destination' : 'Arrived From', osCase.arrived_from || osCase.country_of_departure],
    ['Booked By', osCase.booked_by],
    ['Detained By', osCase.detained_by],
    ['Detention Date', fmtDate(osCase.detention_date)],
    ['D.R. No.', osCase.dr_no],
    ['Seal No.', osCase.seal_no],
    ['Previous O/S Cases', osCase.previous_os_details],
  ];

  return (
    <div className="bg-white rounded-xl border border-slate-200 overflow-hidden">
      <div className="bg-slate-50 px-5 py-3.5 border-b border-slate-200 flex items-center gap-2">
        <User size={16} className="text-blue-600" />
        <h2 className="font-bold text-slate-700 text-sm uppercase tracking-wider">Offence Case Details</h2>
      </div>
      <div className="p-5 grid grid-cols-1 md:grid-cols-2 gap-x-8 gap-y-0 divide-y divide-slate-50">
        {fields.filter(([, val]) => val != null && val !== '').map(([label, val]) => (
          <div key={label} className="py-2.5 flex items-start gap-3">
            <span className="text-xs text-slate-500 uppercase tracking-wide font-semibold w-44 shrink-0 pt-0.5">{label}</span>
            <span className="text-sm text-slate-800 font-medium">{val || <span className="text-slate-300">—</span>}</span>
          </div>
        ))}
        {osCase.pax_address1 && (
          <div className="col-span-2 py-2.5 flex items-start gap-3">
            <span className="text-xs text-slate-500 uppercase tracking-wide font-semibold w-44 shrink-0 pt-0.5">Address</span>
            <span className="text-sm text-slate-800 font-medium">
              {[osCase.pax_address1, osCase.pax_address2, osCase.pax_address3].filter(Boolean).join(', ')}
            </span>
          </div>
        )}
        {osCase.supdts_remarks && (
          <div className="col-span-2 py-2.5 flex items-start gap-3 bg-amber-50 rounded-lg px-3 -mx-3">
            <span className="text-xs text-amber-700 uppercase tracking-wide font-bold w-44 shrink-0 pt-0.5">SDO Remarks</span>
            <span className="text-sm text-slate-800 font-medium leading-relaxed whitespace-pre-wrap">{osCase.supdts_remarks}</span>
          </div>
        )}
      </div>
    </div>
  );
});

// ── ItemsPanel: Read-only display of SDO-categorized items ────────────────────
const ItemsPanel = memo(function ItemsPanel({ osCase }: { osCase: OSCase }) {
  const totalValAfterFA = osCase.items.reduce((s, i) => s + valueAfterFA(i), 0);
  const totalDutyAll = osCase.items.reduce((s, i) => s + (i.items_duty || 0), 0);

  // Compute live category-value breakdown
  let rfVal = 0, refVal = 0, confsVal = 0, underDutyVal = 0, unassignedVal = 0, totalFA = 0;
  const qtyFaItems: string[] = [];
  
  for (const item of osCase.items) {
    const rc = item.items_release_category || 'Under OS';
    const val = valueAfterFA(item);

    if ((item.items_fa_type || 'value') === 'value') {
      const origVal = Number(item.items_value) || 0;
      totalFA += Math.max(0, origVal - val);
    } else if (Number(item.items_fa_qty) > 0) {
      qtyFaItems.push(`${_fmtQty(item.items_fa_qty || 0)} ${_uqcLabel(item.items_fa_uqc || '')} of ${item.items_desc}`);
    }

    if (rc === 'CONFS') confsVal += val;
    else if (rc === 'RF') rfVal += val;
    else if (rc === 'REF') refVal += val;
    else if (rc === 'Under Duty') underDutyVal += val;
    else if (rc === 'Under OS' || !rc) unassignedVal += val;
  }

  return (
    <div className="bg-white rounded-xl border border-slate-200 overflow-hidden">
      <div className="bg-slate-50 px-5 py-3.5 border-b border-slate-200 flex items-center gap-2">
        <Package size={16} className="text-orange-500" />
        <h2 className="font-bold text-slate-700 text-sm uppercase tracking-wider">
          Seized Goods
        </h2>
        <span className="ml-auto text-xs text-slate-500">{osCase.total_items} item(s)</span>
      </div>
      <div className="overflow-auto">
        <table className="w-full text-sm">
          <thead className="text-xs text-slate-500 uppercase bg-slate-50 border-b border-slate-200">
            <tr>
              <th className="px-4 py-3 text-center w-12">S.No</th>
              <th className="px-4 py-3 text-left">Description</th>
              <th className="px-4 py-3 text-left">Duty Type</th>
              <th className="px-4 py-3 text-center">Qty / UQC</th>
              <th className="px-4 py-3 text-right">Rate / Piece (₹)</th>
              <th className="px-4 py-3 text-right">Free Allowance</th>
              <th className="px-4 py-3 text-right bg-amber-50">Value after FA (₹)</th>
              <th className="px-4 py-3 text-center">Rate (%)</th>
              <th className="px-4 py-3 text-right">Duty (₹)</th>
              <th className="px-4 py-3 text-center w-32">Disposal</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-100">
            {osCase.items.map(item => (
              <AdjItemRow key={item.id} item={item} />
            ))}
          </tbody>
          <tfoot className="bg-slate-50 border-t border-slate-200 font-bold">
            <tr>
              <td colSpan={6} className="px-4 py-2 text-right text-xs text-slate-500 uppercase tracking-wider">
                Overall Value (after FA):
              </td>
              <td className="px-4 py-2 text-right text-base text-amber-800">
                ₹{totalValAfterFA.toLocaleString('en-IN', { minimumFractionDigits: 2 })}
              </td>
              <td colSpan={3} />
            </tr>
            <tr className="border-t border-slate-200">
              <td colSpan={8} className="px-4 py-2 text-right text-xs text-slate-500 uppercase tracking-wider">
                Overall Duty:
              </td>
              <td className="px-4 py-2 text-right text-base text-brand-700">
                ₹{totalDutyAll.toLocaleString('en-IN', { minimumFractionDigits: 2 })}
              </td>
              <td />
            </tr>
          </tfoot>
        </table>
      </div>

      {/* Live category-wise Value Summary moved to Order Details panel */}
    </div>
  );
});

// ── Main component ─────────────────────────────────────────────────────────────
export default function AdjudicationForm() {
  const { os_no, os_year } = useParams();
  const navigate = useNavigate();
  const { user } = useAuth();

  const [osCase, setOsCase] = useState<OSCase | null>(null);
  const [loading, setLoading] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const { generateRemark, loading: remarksLoading } = useRemarksGenerator();

  // Contextual questions modal (same flow as OffenceForm SUPDT remarks)
  const [showContextModal, setShowContextModal] = useState(false);
  const [contextQuestions, setContextQuestions] = useState<ContextualQuestion[]>([]);
  const [contextAnswers, setContextAnswers] = useState<ContextualAnswers>({});

  const triggerGenerateOrder = () => {
    const caseItems = osCase?.items || [];
    const questions = detectContextualQuestions(caseItems);
    if (questions.length > 0) {
      setContextQuestions(questions);
      setContextAnswers({});
      setShowContextModal(true);
    } else {
      const text = generateRemark('ADJN', caseItems, {
        pax_name: osCase?.pax_name, flight_no: osCase?.flight_no,
        flight_date: osCase?.flight_date, port_of_dep_dest: osCase?.port_of_dep_dest,
        os_date: osCase?.os_date, case_type: osCase?.case_type,
      }, {});
      setRemarks(text);
    }
  };

  const handleContextSubmit = () => {
    const caseItems = osCase?.items || [];
    const text = generateRemark('ADJN', caseItems, {
      pax_name: osCase?.pax_name, flight_no: osCase?.flight_no,
      flight_date: osCase?.flight_date, port_of_dep_dest: osCase?.port_of_dep_dest,
      os_date: osCase?.os_date, case_type: osCase?.case_type,
    }, contextAnswers);
    setRemarks(text);
    setShowContextModal(false);
  };

  const [error, setError] = useState('');
  const [success, setSuccess] = useState('');

  // Auto-clear success banner after 6 seconds
  useEffect(() => {
    if (!success) return;
    const t = setTimeout(() => setSuccess(''), 3000);
    return () => clearTimeout(t);
  }, [success]);

  // Each adjData field is its own state so only the affected input re-renders
  const [adjDate, setAdjDate]   = useState(() => new Date().toISOString().split('T')[0]);
  const [offrName, setOffrName] = useState(() => user?.user_name || '');
  const [offrDesig, setOffrDesig] = useState(() => user?.user_desig || '');
  const [remarks, setRemarks]   = useState('');
  const [rfAmt, setRfAmt]       = useState(0);
  const [refAmt, setRefAmt]     = useState(0);
  const [ppAmt, setPpAmt]       = useState(0);
  
  const [confirmSave, setConfirmSave] = useState(false);
  const [isReAdjudicating, setIsReAdjudicating] = useState(false);

  // ── Section reference — loaded from admin PIT config, hardcoded fallbacks ────
  // Config keys (set in OS Template Editor → Confiscation Section Reference):
  //   confiscation_section_import / confiscation_section_export
  //   confiscation_fixed_subs_import / confiscation_fixed_subs_export   (csv)
  //   confiscation_optional_subs_import / confiscation_optional_subs_export (csv)
  interface SectionConfig {
    importSection: string; exportSection: string;
    importFixed: string[]; exportFixed: string[];
    importOptional: string[]; exportOptional: string[];
  }
  const parseSubs = (csv: string): string[] =>
    csv.split(',').map(s => s.trim().toLowerCase()).filter(Boolean);

  const [sectionCfg, setSectionCfg] = useState<SectionConfig>({
    importSection: '111', exportSection: '113',
    importFixed: ['d', 'l', 'm'], exportFixed: ['d', 'h', 'i'],
    importOptional: ['i', 'o'], exportOptional: ['e'],
  });

  // Fetch once on mount — always use today so we get the CURRENT legal reference
  // (not the case's os_date; sections apply at adjudication time, not detection time)
  useEffect(() => {
    const today = new Date().toISOString().split('T')[0];
    api.get('/admin/config/pit', { params: { ref_date: today } })
      .then(res => {
        const ptc: Record<string, { field_value: string }> = res.data?.print_template ?? {};
        const get = (key: string, fb: string) => ptc[key]?.field_value?.trim() || fb;
        setSectionCfg({
          importSection:  get('confiscation_section_import',        '111'),
          exportSection:  get('confiscation_section_export',        '113'),
          importFixed:    parseSubs(get('confiscation_fixed_subs_import',    'd,l,m')),
          exportFixed:    parseSubs(get('confiscation_fixed_subs_export',    'd,h,i')),
          importOptional: parseSubs(get('confiscation_optional_subs_import', 'i,o')),
          exportOptional: parseSubs(get('confiscation_optional_subs_export', 'e')),
        });
      })
      .catch(() => { /* keep defaults silently */ });
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const [selectedOptionals, setSelectedOptionals] = useState<string[]>([]);

  // Pre-fill optional chips from saved adjn_section_ref when a case loads/changes.
  // Extracts (x) subsection letters and restores the ones that are still in optionalSubs.
  useEffect(() => {
    if (!osCase?.adjn_section_ref) { setSelectedOptionals([]); return; }
    const saved = (osCase.adjn_section_ref.match(/\(([a-z])\)/g) || [])
      .map((m: string) => m.slice(1, -1));
    // Store all parsed subs — safeOptionals filters to valid ones once sectionCfg loads.
    // Filtering here would silently drop selections if optionalSubs hasn't populated yet (race).
    setSelectedOptionals(saved);
  }, [osCase?.id, osCase?.adjn_section_ref]); // eslint-disable-line react-hooks/exhaustive-deps

  const isExportCase = osCase?.case_type === 'Export Case';
  const sectionNo    = isExportCase ? sectionCfg.exportSection  : sectionCfg.importSection;
  const fixedSubs    = isExportCase ? sectionCfg.exportFixed    : sectionCfg.importFixed;
  const optionalSubs = isExportCase ? sectionCfg.exportOptional : sectionCfg.importOptional;

  // Drop any selection that is no longer in optionalSubs (handles config updates & case type switch)
  const safeOptionals = selectedOptionals.filter(s => optionalSubs.includes(s));

  // Merge fixed + valid optionals, sort alphabetically, format with & before last
  const allSubs = [...fixedSubs, ...safeOptionals].sort();
  const formatSubs = (subs: string[]): string => {
    if (subs.length === 0) return '';
    const parts = subs.map(s => `(${s})`);
    if (parts.length === 1) return parts[0];
    return parts.slice(0, -1).join(', ') + ' & ' + parts[parts.length - 1];
  };
  const sectionText = `Section ${sectionNo}${formatSubs(allSubs)} of the Customs Act, 1962`;

  // Styled confirmation modals (replaces native window.confirm / window.prompt)
  const [showDeleteModal, setShowDeleteModal] = useState(false);

  const REMARKS_MAX = 3000;
  const remarksLen  = remarks.length;
  const { redeemedVal, reExportVal, totalConfsValue, underOsValue, underDutyVal, totalFA, qtyFaItems, totalDutyLive } = useMemo(() => {
    if (!osCase) return { redeemedVal: 0, reExportVal: 0, totalConfsValue: 0, underOsValue: 0, underDutyVal: 0, totalFA: 0, qtyFaItems: [] as string[], totalDutyLive: 0 };
    let rf = 0, ref = 0, confs = 0, uos = 0, duty = 0, fa = 0, dutySum = 0;
    const qFa: string[] = [];

    for (const item of osCase.items) {
      const rc = item.items_release_category || 'Under OS';
      const valAfterFa = valueAfterFA(item);
      dutySum += Number(item.items_duty || 0);

      if ((item.items_fa_type || 'value') === 'value') {
        const origVal = Number(item.items_value) || 0;
        fa += Math.max(0, origVal - valAfterFa);
      } else if (Number(item.items_fa_qty) > 0) {
        qFa.push(`${_fmtQty(item.items_fa_qty || 0)} ${_uqcLabel(item.items_fa_uqc || '')} of ${item.items_desc}`);
      }

      if (rc === 'CONFS') confs += valAfterFa;
      else if (rc === 'RF') rf += valAfterFa;
      else if (rc === 'REF') ref += valAfterFa;
      else if (rc === 'Under Duty') duty += valAfterFa;
      else if (rc === 'Under OS' || !rc) uos += valAfterFa;
    }
    return { redeemedVal: Math.round(rf), reExportVal: Math.round(ref), totalConfsValue: Math.round(confs), underOsValue: Math.round(uos), underDutyVal: Math.round(duty), totalFA: Math.round(fa), qtyFaItems: qFa, totalDutyLive: Math.round(dutySum) };
  }, [osCase]);

  const totalDuty   = totalDutyLive;
  const totalDemand = totalDuty + rfAmt + refAmt + ppAmt;

  useEffect(() => {
    if (!os_no || !os_year) return;
    setLoading(true);
    api.get(`/os/${os_no}/${os_year}`)
      .then(res => {
        const data: OSCase = res.data;
        setOsCase(data);

        if (data.adj_offr_name) {
          setOffrName(data.adj_offr_name || user?.user_name || '');
          setOffrDesig(data.adj_offr_designation || user?.user_desig || '');
          setRemarks(data.adjn_offr_remarks || '');
          setRfAmt(data.rf_amount || 0);
          setRefAmt(data.ref_amount || 0);
          setPpAmt(data.pp_amount || 0);
          // Pre-fill adjudication date from the existing record so that
          // re-adjudication defaults to the original date, not today.
          if (data.adjudication_date) setAdjDate(data.adjudication_date);
          // closure_ind is always set automatically on adjudication
        }
        setLoading(false);
      })
      .catch(err => {
        setError(err.response?.data?.detail || 'Failed to load case');
        setLoading(false);
      });
  }, [os_no, os_year, user]);

  // Ctrl+S / Cmd+S → trigger save
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 's') {
        e.preventDefault();
        if (confirmSave) handleSave();
        else setError('Please tick the confirmation checkbox before saving.');
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [confirmSave]);  // eslint-disable-line react-hooks/exhaustive-deps

  const handleSave = async () => {
    if (!confirmSave) { setError('Please tick the confirmation checkbox before saving.'); return; }
    if (!offrName.trim()) { setError('Adjudicating Officer Name is required.'); return; }
    if (!offrDesig.trim()) { setError('Adjudicating Officer Designation is required.'); return; }
    if (remarksLen > REMARKS_MAX) { setError(`Remarks exceeds ${REMARKS_MAX} character limit.`); return; }
    
    // Validate: if any Under OS items exist, they must all be categorized by SDO
    if (osCase) {
      const uncategorized = osCase.items.filter(i => i.items_release_category === 'Under OS' || !i.items_release_category);
      if (uncategorized.length > 0) {
        setError(`Cannot adjudicate. SDO left ${uncategorized.length} item(s) as "Under OS" without specifying a disposal category (RF/REF/CONFS). Return case to SDO to fix.`);
        return;
      }
    }
    // Validate: RF items require rf_amount > 0
    if (redeemedVal > 0 && rfAmt <= 0) {
      setError('You have items under Redemption Fine. Please enter the R.F. Amount.');
      return;
    }
    // Validate: REF items require ref_amount > 0
    if (reExportVal > 0 && refAmt <= 0) {
      setError('You have items under Re-Export Fine. Please enter the R.E.F. Amount.');
      return;
    }

    setError('');
    setSubmitting(true);
    try {
      // POST now returns the full updated case — no follow-up GET needed
      const { data: updatedCase } = await api.post(`/os/${os_no}/${os_year}/adjudicate`, {
        adj_offr_name: offrName,
        adj_offr_designation: offrDesig,
        adjudication_date: adjDate,
        adjn_offr_remarks: remarks,
        adjn_section_ref: sectionText,
        rf_amount: rfAmt,
        ref_amount: refAmt,
        pp_amount: ppAmt,
        confiscated_value: totalConfsValue,
        redeemed_value: redeemedVal,
        re_export_value: reExportVal,
        close_case: true,
      });
      setOsCase(updatedCase);
      setSuccess(
        isReAdjudicating
          ? 'Adjudication updated successfully. The new adjudication order is now active.'
          : 'Case adjudicated successfully. You can now print the O.S. using the button above.'
      );
      setIsReAdjudicating(false);
      setConfirmSave(false);
    } catch (err: any) {
      let detail = err.response?.data?.detail || 'Failed to save adjudication.';
      if (Array.isArray(detail)) detail = detail.map((e: any) => `${e.loc?.join('.')} - ${e.msg}`).join(', ');
      else if (typeof detail === 'object') detail = JSON.stringify(detail);
      setError(detail);
    } finally {
      setSubmitting(false);
    }
  };

  const handlePrint = async () => {
    setSubmitting(true);
    try {
      const pdfData = await api.get(`/os/${os_no}/${os_year}/print-pdf`, { responseType: 'arraybuffer' }).then((r) => r.data);
      
      try {
        const { save } = await import('@tauri-apps/plugin-dialog');
        const { writeFile } = await import('@tauri-apps/plugin-fs');
        const savePath = await save({
          title: 'Save OS Print',
          defaultPath: `OS_${os_no}_${os_year}.pdf`,
          filters: [{ name: 'PDF', extensions: ['pdf'] }],
        });
        if (savePath) {
          await writeFile(savePath, new Uint8Array(pdfData));
          showDownloadToast(`PDF saved to ${savePath}`);
        }
      } catch {
        const blob = new Blob([pdfData], { type: 'application/pdf' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = `OS_${os_no}_${os_year}.pdf`;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        setTimeout(() => URL.revokeObjectURL(url), 5000);
        showDownloadToast(`PDF downloaded as OS_${os_no}_${os_year}.pdf`);
      }
    } catch (err: any) {
      let detail = err.response?.data?.detail || 'Failed to download OS PDF.';
      if (Array.isArray(detail)) detail = detail.map((e: any) => `${e.loc?.join('.')} - ${e.msg}`).join(', ');
      else if (typeof detail === 'object') detail = JSON.stringify(detail);
      setError(detail);
    } finally {
      setSubmitting(false);
    }
  };

  const handleDeleteOS = () => {
    setError('');
    setShowDeleteModal(true);
  };

  const confirmDeleteOS = async () => {
    setShowDeleteModal(false);
    setSubmitting(true);
    try {
      await api.post(`/os/${os_no}/${os_year}/quash`);
      setSuccess('O.S. Case permanently deleted.');
      setTimeout(() => navigate('/adjudication/adjudicated'), 800);
    } catch (err: any) {
      let detail = err.response?.data?.detail || 'Failed to delete case.';
      if (Array.isArray(detail)) detail = detail.map((e: any) => `${e.loc?.join('.')} - ${e.msg}`).join(', ');
      else if (typeof detail === 'object') detail = JSON.stringify(detail);
      setError(detail);
      setSubmitting(false);
    }
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-center space-y-3">
          <Gavel size={36} className="text-amber-400 mx-auto animate-pulse" />
          <p className="text-slate-600">Loading case details...</p>
        </div>
      </div>
    );
  }

  if (!osCase) {
    return (
      <div className="bg-red-50 border border-red-200 rounded-lg p-6 text-red-700 flex items-start gap-3">
        <AlertCircle size={20} />
        <p>{error || 'Case not found.'}</p>
      </div>
    );
  }

  // IMPORTANT: Must match backend _pending_filters() in offence.py.
  // A case is adjudicated if EITHER field is set — locks the form to VIEW ONLY.
  const isAlreadyAdjudicated = !!(osCase.adjudication_date || osCase.adj_offr_name);
  // 24-hour modification window: starts from adjudication_time, lasts exactly 24 hours
  const canModify = osCase.adjudication_time
    ? (new Date().getTime() - new Date(osCase.adjudication_time).getTime() < 86400000)
    : true;

  return (
    <div className="space-y-5 max-w-full mx-auto">

      {/* Page Header */}
      <div className="bg-amber-800 text-white px-5 py-4 rounded-xl flex items-center justify-between border border-amber-700">
        <div className="flex items-center gap-3">
          <button onClick={() => navigate('/adjudication/pending')} className="p-2 bg-white/10 hover:bg-white/20 rounded-lg transition-colors">
            <ArrowLeft size={18} />
          </button>
          <div>
            <h1 className="text-lg font-bold">Adjudication — O/S No. {osCase.os_no}/{osCase.os_year}</h1>
            <p className="text-amber-200 text-xs">
              Registered on {fmtDate(osCase.os_date)} · {osCase.booked_by}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-3">
          {/* Edit SDO details: available before adjudication OR within the 24h window (not in re-adjudication mode) */}
          {(!isAlreadyAdjudicated || (isAlreadyAdjudicated && canModify && !isReAdjudicating)) && (
            <button
              onClick={() => navigate(`/adjudication/edit-sdo/${osCase.os_no}/${osCase.os_year}`)}
              className="flex items-center gap-1.5 bg-white/10 hover:bg-white/20 text-white text-xs px-3 py-1.5 rounded-lg transition-colors border border-white/20"
              title="Edit SDO Inputs"
            >
              <Edit size={14} /> Edit Case Details
            </button>
          )}
          {isAlreadyAdjudicated && (
            <div className="flex items-center gap-2">
              <span className="bg-green-500/20 border border-green-400/40 text-green-100 text-xs font-semibold px-3 py-1.5 rounded-full flex items-center gap-1.5">
                <CheckCircle size={13} /> Adjudicated
              </span>
              {canModify && !isReAdjudicating && (
                <>
                  <button
                    onClick={() => { setIsReAdjudicating(true); setConfirmSave(false); setError(''); }}
                    className="flex items-center gap-1.5 bg-amber-500/20 hover:bg-amber-500/40 text-amber-100 text-xs px-3 py-1.5 rounded-lg transition-colors border border-amber-400/40 font-bold"
                    title="Modify adjudication order (within 24-hour window)"
                  >
                    <Edit size={12} /> Edit Adjudication
                  </button>
                  <button
                    onClick={handleDeleteOS}
                    disabled={submitting}
                    className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-lg transition-colors font-bold bg-red-700 hover:bg-red-600 border border-red-500 text-white disabled:opacity-50"
                    title="Permanently delete this O.S. case (within 24-hour window)"
                  >
                    <Trash2 size={12} /> Delete OS
                  </button>
                </>
              )}
              {isReAdjudicating && (
                <span className="bg-amber-400/20 border border-amber-300/40 text-amber-100 text-xs font-semibold px-3 py-1.5 rounded-full flex items-center gap-1.5">
                  <Clock size={12} /> Re-Adjudication Mode
                </span>
              )}
              {!canModify && (
                <span className="text-white/40 text-xs font-medium">24h window closed</span>
              )}
            </div>
          )}
        </div>
      </div>

      {/* Alerts */}
      {error && (
        <div className="bg-red-50 border border-red-200 text-red-700 p-4 rounded-lg flex items-start gap-3">
          <AlertCircle size={18} className="shrink-0 mt-0.5" />
          <p className="text-sm font-medium">{error}</p>
        </div>
      )}
      {success && (
        <div className="bg-green-50 border border-green-200 text-green-700 p-4 rounded-lg flex flex-col sm:flex-row sm:items-center justify-between gap-3">
          <div className="flex items-start gap-3">
            <CheckCircle size={18} className="shrink-0 mt-0.5" />
            <div className="text-sm font-medium">
              <p>{success}</p>
              {success.includes('adjudicated successfully') && (
                <p className="mt-1 text-xs text-green-800 font-bold">
                  Note: You have 24 hours from adjudication to edit the case details, modify the adjudication order, or delete it entirely.
                </p>
              )}
            </div>
          </div>
          <button
            onClick={() => navigate('/adjudication/pending')}
            className="shrink-0 flex items-center gap-1.5 bg-white border border-green-300 text-green-700 text-xs font-semibold px-4 py-2 rounded-lg hover:bg-green-50 transition-colors"
          >
            Go to Pending List
          </button>
        </div>
      )}

      {/* READ-ONLY panels — CaseDetailsPanel and ItemsPanel */}
      <CaseDetailsPanel osCase={osCase} />
      <ItemsPanel osCase={osCase} />

      {/* WRITABLE: Adjudication Order Details */}
      <div className="bg-white rounded-xl border-2 border-amber-300 overflow-hidden">
        <div className="bg-amber-700 px-5 py-3.5 flex items-center gap-2">
          <Gavel size={16} className="text-amber-200" />
          <h2 className="font-bold text-white text-sm uppercase tracking-wider">Adjudication Order Details</h2>
          {isAlreadyAdjudicated && !isReAdjudicating && (
            <span className="ml-auto text-xs bg-red-500/20 text-red-200 border border-red-400/30 px-2 py-0.5 rounded-full font-bold">
              VIEW ONLY — Previously Adjudicated
            </span>
          )}
          {isReAdjudicating && (
            <span className="ml-auto text-xs bg-amber-400/20 text-amber-200 border border-amber-300/30 px-2 py-0.5 rounded-full font-bold">
              EDITING — Re-Adjudication (24h window)
            </span>
          )}
        </div>

        <fieldset disabled={isAlreadyAdjudicated && !isReAdjudicating} className="p-6 space-y-6">
          {/* Officer + Date */}
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <div>
              <label className="block text-xs font-bold text-amber-800 uppercase tracking-wider mb-1.5">Adjudication Date *</label>
              <DatePicker
                id="input-adjn-date"
                value={adjDate}
                onChange={setAdjDate}
                inputClassName="w-full px-3 py-2.5 border border-amber-300 rounded-lg bg-amber-50 focus:ring-2 focus:ring-amber-500 focus:border-amber-500 text-slate-800 font-medium"
              />
            </div>
            <div>
              <label className="block text-xs font-bold text-amber-800 uppercase tracking-wider mb-1.5">Adjudicating Officer Name *</label>
              <input
                id="input-offr-name"
                type="text"
                className="w-full px-3 py-2.5 border border-amber-300 rounded-lg bg-amber-50 focus:ring-2 focus:ring-amber-500 focus:border-amber-500 text-slate-800 font-medium"
                value={offrName}
                onChange={e => setOffrName(e.target.value.toUpperCase())}
                placeholder="Officer Name"
              />
            </div>
            <div>
              <label className="block text-xs font-bold text-amber-800 uppercase tracking-wider mb-1.5">Designation *</label>
              <input
                id="input-offr-desig"
                type="text"
                className="w-full px-3 py-2.5 border border-amber-300 rounded-lg bg-amber-50 focus:ring-2 focus:ring-amber-500 focus:border-amber-500 text-slate-800 font-medium"
                value={offrDesig}
                onChange={e => setOffrDesig(e.target.value.toUpperCase())}
                placeholder="Officer Designation"
              />
            </div>
          </div>

          {/* Financial Demands & Case Values */}
          <div>
            <h3 className="flex items-center gap-2 text-xs font-bold text-amber-800 uppercase tracking-wider mb-3 border-b border-amber-100 pb-2">
              <FileText size={13} /> Case Values & Demands (₹)
            </h3>
            
            <div className="bg-slate-50 border border-slate-200 rounded-xl p-4 mb-4">
              <div className="flex items-center justify-between mb-4">
                 <h4 className="text-[11px] font-bold text-slate-500 uppercase">Goods Value Breakdown</h4>
                 {underOsValue > 0 && (
                   <div className="text-[10px] text-orange-600 flex items-center gap-1.5 font-bold bg-orange-50 px-3 py-1 rounded-full border border-orange-200">
                       <AlertCircle size={12} />
                       Under OS (Unassigned): ₹{underOsValue.toLocaleString('en-IN', { minimumFractionDigits: 2 })}
                   </div>
                 )}
              </div>
              
              <div className="flex flex-wrap gap-3">
                {underDutyVal > 0 && (
                  <div className="bg-emerald-50 border border-emerald-200 rounded-lg px-3 py-2 flex-grow min-w-[120px]">
                    <div className="text-[10px] text-emerald-700 font-bold uppercase">Under Duty</div>
                    <div className="text-sm font-black text-emerald-800">₹{underDutyVal.toLocaleString('en-IN', { minimumFractionDigits: 2 })}</div>
                  </div>
                )}
                {redeemedVal > 0 && (
                  <div className="bg-green-50 border border-green-200 rounded-lg px-3 py-2 flex-grow min-w-[120px]">
                    <div className="text-[10px] text-green-700 font-bold uppercase">Redemption (RF)</div>
                    <div className="text-sm font-black text-green-800">₹{redeemedVal.toLocaleString('en-IN', { minimumFractionDigits: 2 })}</div>
                  </div>
                )}
                {reExportVal > 0 && (
                  <div className="bg-blue-50 border border-blue-200 rounded-lg px-3 py-2 flex-grow min-w-[120px]">
                    <div className="text-[10px] text-blue-700 font-bold uppercase">Re-Export (REF)</div>
                    <div className="text-sm font-black text-blue-800">₹{reExportVal.toLocaleString('en-IN', { minimumFractionDigits: 2 })}</div>
                  </div>
                )}
                {totalConfsValue > 0 && (
                  <div className="bg-red-50 border border-red-200 rounded-lg px-3 py-2 flex-grow min-w-[120px]">
                    <div className="text-[10px] text-red-600 font-bold uppercase">Abs. Confiscation</div>
                    <div className="text-sm font-black text-red-800">₹{totalConfsValue.toLocaleString('en-IN', { minimumFractionDigits: 2 })}</div>
                  </div>
                )}
                {(totalFA > 0 || qtyFaItems.length > 0) && (
                  <div className="bg-indigo-50 border border-indigo-200 rounded-lg px-3 py-2 max-w-[280px]">
                    <div className="text-[10px] text-indigo-700 font-bold uppercase">Free Allowance</div>
                    {totalFA > 0 && (
                      <div className="text-sm font-black text-indigo-800">
                        ₹{totalFA.toLocaleString('en-IN', { minimumFractionDigits: 2 })}
                      </div>
                    )}
                    {qtyFaItems.length > 0 && (
                      <div className="text-[10px] text-indigo-600 font-bold leading-tight mt-1 truncate whitespace-normal">
                        {totalFA > 0 ? 'along with ' : ''}{qtyFaItems.join(' & ')}
                      </div>
                    )}
                  </div>
                )}
              </div>
            </div>

            <h4 className="text-[11px] font-bold text-slate-500 uppercase mt-5 mb-3">Imposed Fines & Penalties</h4>
            <div className="grid grid-cols-1 md:grid-cols-4 gap-4">
              <div>
                <label className={`block text-xs font-semibold mb-1.5 ${redeemedVal > 0 ? 'text-slate-600' : 'text-slate-400'}`}>Redemption Fine (RF)</label>
                <div className="relative">
                  <span className={`absolute left-3 top-1/2 -translate-y-1/2 ${redeemedVal > 0 ? 'text-slate-400' : 'text-slate-300'}`}>₹</span>
                  <input id="input-rf" type="number" min={0} placeholder="0"
                    disabled={redeemedVal === 0}
                    className="w-full pl-7 pr-3 py-2.5 border border-slate-300 rounded-lg bg-slate-50 focus:ring-2 focus:ring-amber-500 text-right font-bold text-slate-800 placeholder-slate-400 disabled:bg-slate-100 disabled:text-slate-400 disabled:border-slate-200 transition-colors"
                    value={rfAmt || ''} onChange={e => setRfAmt(Number(e.target.value))} />
                </div>
              </div>
              <div>
                <label className={`block text-xs font-semibold mb-1.5 ${reExportVal > 0 ? 'text-slate-600' : 'text-slate-400'}`}>Re-Export Fine (REF)</label>
                <div className="relative">
                  <span className={`absolute left-3 top-1/2 -translate-y-1/2 ${reExportVal > 0 ? 'text-slate-400' : 'text-slate-300'}`}>₹</span>
                  <input id="input-ref" type="number" min={0} placeholder="0"
                    disabled={reExportVal === 0}
                    className="w-full pl-7 pr-3 py-2.5 border border-slate-300 rounded-lg bg-slate-50 focus:ring-2 focus:ring-amber-500 text-right font-bold text-slate-800 placeholder-slate-400 disabled:bg-slate-100 disabled:text-slate-400 disabled:border-slate-200 transition-colors"
                    value={refAmt || ''} onChange={e => setRefAmt(Number(e.target.value))} />
                </div>
              </div>
              <div>
                <label className="block text-xs font-semibold text-slate-600 mb-1.5">Personal Penalty (PP)</label>
                <div className="relative">
                  <span className="absolute left-3 top-1/2 -translate-y-1/2 text-slate-400">₹</span>
                  <input id="input-pp" type="number" min={0} placeholder="0"
                    className="w-full pl-7 pr-3 py-2.5 border border-slate-300 rounded-lg bg-slate-50 focus:ring-2 focus:ring-amber-500 text-right font-bold text-slate-800 placeholder-slate-400"
                    value={ppAmt || ''} onChange={e => setPpAmt(Number(e.target.value))} />
                </div>
              </div>
              <div>
                <label className="block text-xs font-bold text-red-700 mb-1.5 uppercase">Total to be Paid</label>
                <div className="px-3 py-2.5 bg-red-50 border-2 border-red-300 rounded-lg text-right font-bold text-red-700 text-base flex flex-col items-end">
                  <span>₹{totalDemand.toLocaleString('en-IN', { minimumFractionDigits: 2 })}</span>
                  <span className="text-[10px] text-red-500 font-medium">
                    Incl. SDO Duty: ₹{totalDuty.toLocaleString('en-IN', { minimumFractionDigits: 2 })}
                  </span>
                </div>
              </div>
            </div>
          </div>

          {/* Gist / Remarks */}
          <div className="bg-white rounded-xl border border-amber-200 mt-6 shadow-sm overflow-hidden flex flex-col">
            <div className="p-4 border-b border-amber-200 bg-amber-50 flex justify-between items-center">
              <h2 className="text-sm font-bold text-amber-800 uppercase tracking-wider flex items-center">
                <FileText className="mr-2 text-amber-600" size={16} /> Gist / Remarks of Adjudicating Officer
              </h2>
              <button
                type="button"
                onClick={(e) => { e.preventDefault(); triggerGenerateOrder(); }}
                disabled={remarksLoading || !osCase?.items?.length}
                title={remarksLoading ? 'Loading legal statutes…' : 'Auto-generate order remarks from items'}
                className="text-[11px] px-3 py-1.5 bg-white text-amber-700 hover:bg-amber-100 border border-amber-300 rounded font-bold flex items-center transition-colors shadow-sm disabled:opacity-60 uppercase tracking-wider"
              >
                <Wand2 size={13} className="mr-1.5" />
                {remarksLoading ? 'Loading Statutes…' : 'Auto-Generate Order ✨'}
              </button>
            </div>
            <div className="p-4 bg-white">

              {/* ── Section reference (Section 113 / 111 subsections) ────────── */}
              {osCase && (
                <div className="mb-4 pb-3 border-b border-amber-100">
                  <div className="flex flex-wrap items-center gap-2 mb-2">
                    <span className="text-xs font-bold text-slate-500 uppercase tracking-wider mr-1">
                      Liable Under
                    </span>
                    {/* Section number badge */}
                    <span className="text-xs font-bold text-amber-800 bg-amber-100 border border-amber-300 px-2 py-0.5 rounded">
                      Section {sectionNo}
                    </span>
                    {/* Fixed subsections — always present, non-removable */}
                    {fixedSubs.map(s => (
                      <span
                        key={s}
                        title="Fixed subsection"
                        className="text-xs font-bold text-slate-700 bg-slate-100 border border-slate-300 px-2 py-0.5 rounded select-none"
                      >
                        ({s})
                      </span>
                    ))}
                    {/* Optional subsections — toggleable */}
                    {optionalSubs.map(s => {
                      const active = safeOptionals.includes(s);
                      return (
                        <button
                          key={s}
                          type="button"
                          title={active ? `Remove (${s})` : `Add (${s})`}
                          onClick={() => setSelectedOptionals(prev =>
                            active ? prev.filter(x => x !== s) : [...prev, s]
                          )}
                          className={`text-xs font-bold px-2 py-0.5 rounded border transition-colors ${
                            active
                              ? 'bg-amber-600 text-white border-amber-700 shadow-sm'
                              : 'bg-white text-slate-400 border-dashed border-slate-300 hover:bg-amber-50 hover:text-amber-700 hover:border-amber-400'
                          }`}
                        >
                          ({s})
                        </button>
                      );
                    })}
                  </div>
                  {/* Preview of the fully-formatted section string */}
                  <p className="text-xs text-slate-500 font-mono leading-relaxed">
                    <span className="font-semibold text-amber-700">{sectionText}</span>
                    {' '}— click the dashed buttons above to include optional subsections
                  </p>
                </div>
              )}

              <div className="flex items-center justify-between mb-1.5">
                <label className="block text-xs font-bold text-slate-500 uppercase tracking-wider">
                  Order Findings
                </label>
                <span className={`text-xs font-semibold ${remarksLen > REMARKS_MAX ? 'text-red-600' : remarksLen > REMARKS_MAX * 0.85 ? 'text-orange-500' : 'text-slate-400'}`}>
                  {remarksLen} / {REMARKS_MAX}
                </span>
              </div>
              <textarea
                id="input-gist"
                className={`w-full px-4 py-3 border rounded-lg bg-amber-50 focus:ring-2 focus:ring-amber-500 focus:border-amber-500 text-slate-800 text-sm leading-relaxed resize-none ${remarksLen > REMARKS_MAX ? 'border-red-400' : 'border-amber-300'}`}
                rows={6}
                placeholder="Click the Auto-Generate button to construct the order based on items, or type manually..."
                value={remarks}
                onChange={e => setRemarks(e.target.value.slice(0, REMARKS_MAX))}
              />
            </div>
          </div>

          {(!isAlreadyAdjudicated || isReAdjudicating) && (
            <div className="border-t border-amber-100 pt-4">
              <label className="flex items-center gap-2.5 cursor-pointer select-none">
                <input
                  type="checkbox"
                  id="chk-confirm-adjn"
                  className="w-4 h-4 rounded border-slate-300 text-amber-600 focus:ring-amber-500"
                  checked={confirmSave}
                  onChange={e => setConfirmSave(e.target.checked)}
                />
                <span className="text-sm font-medium text-slate-700">
                  {isReAdjudicating
                    ? 'I confirm the above modifications are correct and wish to save the updated adjudication order.'
                    : 'I confirm the above details are correct and wish to save the adjudication order.'}
                </span>
              </label>
            </div>
          )}
        </fieldset>

        <div className="p-6 pt-0">
          {/* Action Buttons */}
          <div className="flex items-center gap-3 border-t border-amber-100 pt-4">
            {/* First-time adjudication */}
            {!isAlreadyAdjudicated && (
              <button
                id="btn-save-adjn"
                onClick={handleSave}
                disabled={submitting || remarksLen > REMARKS_MAX || !confirmSave}
                className="flex items-center gap-2 bg-amber-700 hover:bg-amber-600 text-white px-6 py-3 rounded-lg font-semibold transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              >
                <Save size={17} />
                {submitting ? 'Saving...' : 'Complete Adjudication'}
              </button>
            )}
            {/* Re-adjudication mode (within 24h window) */}
            {isAlreadyAdjudicated && isReAdjudicating && (
              <>
                <button
                  id="btn-save-readjn"
                  onClick={handleSave}
                  disabled={submitting || remarksLen > REMARKS_MAX || !confirmSave}
                  className="flex items-center gap-2 bg-amber-700 hover:bg-amber-600 text-white px-6 py-3 rounded-lg font-semibold transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  <Save size={17} />
                  {submitting ? 'Saving...' : 'Save Updated Adjudication'}
                </button>
                <button
                  onClick={() => { setIsReAdjudicating(false); setConfirmSave(false); setError(''); }}
                  disabled={submitting}
                  className="flex items-center gap-2 bg-white hover:bg-slate-50 text-slate-700 border border-slate-300 px-6 py-3 rounded-lg font-semibold transition-colors disabled:opacity-50"
                >
                  <XCircle size={17} /> Cancel Edit
                </button>
              </>
            )}
            {/* View mode after adjudication — print only */}
            {isAlreadyAdjudicated && !isReAdjudicating && (
              <button
                onClick={handlePrint}
                disabled={submitting}
                className="flex items-center gap-2 bg-emerald-700 hover:bg-emerald-600 text-white px-6 py-3 rounded-lg font-semibold transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              >
                <Printer size={17} />
                Download OS
              </button>
            )}
            <button
              onClick={() => navigate(-1)}
              className="flex items-center gap-2 bg-white hover:bg-slate-50 text-slate-700 border border-slate-300 px-6 py-3 rounded-lg font-semibold transition-colors"
            >
              <XCircle size={17} />
              {isAlreadyAdjudicated ? 'Back' : 'Cancel'}
            </button>
          </div>
        </div>
      </div>

      {/* Contextual questions modal */}
      {showContextModal && (
        <div className="fixed inset-0 bg-slate-900/50 flex items-center justify-center z-50">
          <div className="bg-white rounded-xl shadow-2xl border border-slate-200 w-full max-w-lg p-6 space-y-5">
            <div>
              <h3 className="text-base font-bold text-slate-800">Additional Information Required</h3>
              <p className="text-xs text-slate-500 mt-1">Please answer the following to generate accurate order remarks.</p>
            </div>
            <div className="space-y-4">
              {contextQuestions.map(q => (
                <div key={q.key} className="border border-slate-200 rounded-lg p-4 bg-slate-50">
                  <p className="text-sm font-medium text-slate-700 mb-3">{q.question}</p>
                  <div className="flex gap-3">
                    <button type="button"
                      onClick={() => setContextAnswers(prev => ({ ...prev, [q.key]: true }))}
                      className={`flex-1 py-2 px-3 rounded-lg border text-sm font-semibold transition-colors ${contextAnswers[q.key] === true ? 'bg-green-600 text-white border-green-600' : 'bg-white text-slate-700 border-slate-300 hover:border-green-400'}`}
                    >{q.yesLabel || 'Yes'}</button>
                    <button type="button"
                      onClick={() => setContextAnswers(prev => ({ ...prev, [q.key]: false }))}
                      className={`flex-1 py-2 px-3 rounded-lg border text-sm font-semibold transition-colors ${contextAnswers[q.key] === false ? 'bg-red-600 text-white border-red-600' : 'bg-white text-slate-700 border-slate-300 hover:border-red-400'}`}
                    >{q.noLabel || 'No'}</button>
                  </div>
                </div>
              ))}
            </div>
            <div className="flex gap-3 pt-1">
              <button type="button" onClick={() => setShowContextModal(false)}
                className="flex-1 py-2 px-4 rounded-lg border border-slate-300 text-slate-700 text-sm font-medium hover:bg-slate-50">
                Cancel
              </button>
              <button type="button"
                disabled={contextQuestions.some(q => contextAnswers[q.key] === undefined)}
                onClick={handleContextSubmit}
                className="flex-1 py-2 px-4 rounded-lg bg-amber-700 text-white text-sm font-bold hover:bg-amber-600 disabled:opacity-50 flex items-center justify-center gap-1.5">
                <Wand2 size={14} /> Generate Order
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ── Delete OS confirmation modal ──────────────────────────────────── */}
      {showDeleteModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm">
          <div className="bg-white rounded-2xl shadow-2xl w-full max-w-md mx-4 overflow-hidden">
            <div className="bg-red-700 px-6 py-4 flex items-center gap-3">
              <AlertTriangle size={22} className="text-white shrink-0" />
              <h2 className="text-white font-bold text-base">Delete O.S. Case</h2>
            </div>
            <div className="px-6 py-5 space-y-3">
              <p className="text-slate-800 font-semibold text-sm">
                Are you sure you want to permanently delete O.S.&nbsp;No.&nbsp;
                <span className="text-red-700 font-bold">{os_no}/{os_year}</span>?
              </p>
              <p className="text-slate-600 text-sm leading-relaxed">
                All records will be removed as if this case never existed.
                This action <strong>cannot be undone</strong>.
              </p>
            </div>
            <div className="px-6 pb-5 flex gap-3">
              <button
                onClick={() => setShowDeleteModal(false)}
                className="flex-1 py-2.5 px-4 rounded-lg border border-slate-300 text-slate-700 text-sm font-semibold hover:bg-slate-50 transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={confirmDeleteOS}
                className="flex-1 py-2.5 px-4 rounded-lg bg-red-700 hover:bg-red-600 text-white text-sm font-bold transition-colors flex items-center justify-center gap-2"
              >
                <Trash2 size={15} /> Yes, Delete
              </button>
            </div>
          </div>
        </div>
      )}

    </div>
  );
}
