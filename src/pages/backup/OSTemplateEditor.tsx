import { useState, useEffect, useCallback } from 'react';
import { Pencil, X, Save, Clock, ChevronDown, ChevronUp, PlaneLanding, PlaneTakeoff } from 'lucide-react';
import api from '@/lib/api';

interface PtcRow {
  id: number;
  field_key: string;
  field_label: string;
  field_value: string;
  effective_from: string;
  created_by: string;
}

interface EditState {
  key: string;         // resolved storage key (may be export_* prefixed)
  label: string;
  currentValue: string;
  newValue: string;
  effectiveFrom: string;
  isTemplate: boolean; // has {placeholder} syntax
}

interface OSTemplateEditorProps {
  adminToken: string;
}

type CaseMode = 'arrival' | 'export';

const TODAY = new Date().toISOString().split('T')[0];

// Base keys whose text contains {placeholder} tokens
const PLACEHOLDER_BASE_KEYS = new Set([
  'order_para_rf', 'order_para_ref', 'order_para_abs_conf', 'order_para_pp',
]);

// Arrival base key → export-specific storage key (only for fields that differ)
const EXPORT_KEY_MAP: Record<string, string> = {
  inventory_heading:   'export_inventory_heading',
  waiver_text_1:       'export_waiver_text_1',
  legal_para_1:        'export_legal_para_1',
  legal_para_2:        'export_legal_para_2',
  order_para_rf:       'export_order_para_rf',
  order_para_abs_conf: 'export_order_para_abs_conf',
  order_para_pp:       'export_order_para_pp',
};

// Reverse: export_* key → arrival base key
const EXPORT_KEY_REVERSE: Record<string, string> = Object.fromEntries(
  Object.entries(EXPORT_KEY_MAP).map(([base, exp]) => [exp, base])
);

// Human-readable labels for export-specific keys (shown in edit panel header)
const EXPORT_KEY_LABELS: Record<string, string> = {
  export_inventory_heading:   'Inventory Heading (Export Cases)',
  export_waiver_text_1:       'Waiver Text 1 (Export Cases)',
  export_legal_para_1:        'Legal Paragraph 1 (Export Cases)',
  export_legal_para_2:        'Legal Paragraph 2 (Export Cases)',
  export_order_para_rf:       'Order: Redemption Fine (Export Cases)',
  export_order_para_abs_conf: 'Order: Absolute Confiscation (Export Cases)',
  export_order_para_pp:       'Order: Personal Penalty (Export Cases)',
};

// Hard-coded defaults (arrival)
const ARRIVAL_DEFAULTS: Record<string, string> = {
  inventory_heading:   'INVENTORY OF THE GOODS IMPORTED',
  waiver_text_1:       'The Charges have been orally communicated to me in respect of the goods mentioned overleaf and imported by me. Orders in the case may please be passed without issue of Show Cause Notice. However I may kindly be given a Personal Hearing.',
  legal_para_1:        "In terms of Foreign Trade Policy notified by the Government in pursuance to Section 3(1) & 3(2) of the Foreign Trade (Development & Regulation) Act, 1992 read with the Rules framed thereunder, also read with Section 11(2)(u) of Customs Act, 1962, import of 'goods in commercial quantity / goods in the nature of non-bonafide baggage' is not permitted without a valid import licence, though exemption exists under clause 3(h) of the Foreign Trade (Exemption from application of Rules in certain cases) order 1993 for import of goods by a passenger from abroad only to the extent admissible under the Baggage Rules framed under Section 79 of the Customs Act, 1962.",
  legal_para_2:        "Import of goods non-declared / misdeclared / concealed / in trade and in commercial quantity / non-bonafide in excess of the baggage allowance is therefore liable for confiscation under Section 111(d), (i), (l), (m) & (o) of the Customs Act, 1962 read with Section 3(3) of the Foreign Trade (Development & Regulation) Act, 1992.",
  order_para_rf:       'I Order confiscation of the goods{rf_slnos_text} valued at Rs.{conf_value}/- under Section 111(d), (i), (l), (m) & (o) of the Customs Act, 1962 read with Section 3(3) of Foreign Trade (D&R) Act, 1992, but allow the passenger an option to redeem the goods valued at Rs.{conf_value}/- on a fine of Rs.{rf_amount}/- (Rupees {rf_words} Only) in lieu of confiscation under Section 125 of the Customs Act 1962 within 7 days from the date of receipt of this Order, Duty extra.',
  order_para_ref:      'However, I give an option to reship the goods{ref_slnos_text} valued at Rs.{re_exp_value}/- on a fine of Rs.{ref_amount}/- (Rupees {ref_words} Only) under Section 125 of the Customs Act 1962 within 1 Month from the date of this Order.',
  order_para_abs_conf: 'I {also_text}order absolute confiscation of the goods{abs_conf_slnos_text} valued at Rs.{abs_conf_value}/- under Section 111(d), (i), (l), (m) & (o) of the Customs Act, 1962 read with Section 3(3) of the Foreign Trade (D&R) Act, 1992.',
  order_para_pp:       'I further impose a Personal Penalty of Rs.{pp_amount}/- (Rupees {pp_words} Only) under Section 112(a) of the Customs Act, 1962.',
};

// Hard-coded defaults (export)
const EXPORT_DEFAULTS: Record<string, string> = {
  inventory_heading:   'INVENTORY OF THE GOODS DETAINED FOR EXPORT',
  waiver_text_1:       'The Charges have been orally communicated to me in respect of the goods mentioned overleaf and detained at the time of my departure. Orders in the case may please be passed without issue of Show Cause Notice. However I may kindly be given a Personal Hearing.',
  legal_para_1:        'In terms of Foreign Trade Policy notified by the Government in pursuance to Section 3(1) & 3(2) of the Foreign Trade (Development & Regulation) Act, 1992, export of goods without proper Customs declaration or in violation of applicable export regulations / restrictions is prohibited. Passengers are required to declare all goods carried at the time of departure as mandated under Section 40 of the Customs Act, 1962.',
  legal_para_2:        'Export of goods non-declared / misdeclared / concealed / in commercial quantity / contrary to any prohibition or export restriction is therefore liable for confiscation under Section 113 of the Customs Act, 1962 read with Section 3(3) of the Foreign Trade (Development & Regulation) Act, 1992.',
  order_para_rf:       'I Order confiscation of the goods{rf_slnos_text} valued at Rs.{conf_value}/- under Section 113 of the Customs Act, 1962, but allow the passenger an option to redeem the goods valued at Rs.{conf_value}/- on a fine of Rs.{rf_amount}/- (Rupees {rf_words} Only) in lieu of confiscation under Section 125 of the Customs Act 1962 within 7 days from the date of receipt of this Order.',
  order_para_abs_conf: 'I {also_text}order absolute confiscation of the goods{abs_conf_slnos_text} valued at Rs.{abs_conf_value}/- under Section 113 of the Customs Act, 1962.',
  order_para_pp:       'I further impose a Personal Penalty of Rs.{pp_amount}/- (Rupees {pp_words} Only) under Section 114 of the Customs Act, 1962.',
};

// Highlight {placeholder} tokens in template text
function TemplateText({ text }: { text: string }) {
  const parts = text.split(/(\{[^}]+\})/g);
  return (
    <span>
      {parts.map((part, i) =>
        /^\{[^}]+\}$/.test(part)
          ? <span key={i} className="bg-amber-100 text-amber-700 font-mono text-[10px] px-0.5 rounded">{part}</span>
          : <span key={i}>{part}</span>
      )}
    </span>
  );
}

// Clickable editable section wrapper
function ES({ fieldKey, children, onEdit, selected }: {
  fieldKey: string;
  children: React.ReactNode;
  onEdit: (key: string) => void;
  selected: boolean;
}) {
  return (
    <span
      className={`group/es relative cursor-pointer rounded transition-[outline,outline-color] ${
        selected
          ? 'outline outline-2 outline-blue-500 outline-offset-1'
          : 'hover:outline hover:outline-2 hover:outline-dashed hover:outline-blue-400 hover:outline-offset-1'
      }`}
      onClick={e => { e.stopPropagation(); onEdit(fieldKey); }}
      title="Click to edit"
    >
      {children}
      <span className={`absolute -top-3 -right-1 z-10 flex items-center gap-0.5 bg-blue-500 text-white text-[9px] px-1 py-0 rounded transition-opacity ${selected ? 'opacity-100' : 'opacity-0 group-hover/es:opacity-100'}`}>
        <Pencil size={8} /> edit
      </span>
    </span>
  );
}

export default function OSTemplateEditor({ adminToken }: OSTemplateEditorProps) {
  const [rows, setRows] = useState<PtcRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState('');
  const [edit, setEdit] = useState<EditState | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveMsg, setSaveMsg] = useState('');
  const [showHistory, setShowHistory] = useState(false);
  const [mode, setMode] = useState<CaseMode>('arrival');

  const adminHeaders = () => ({ Authorization: `Bearer ${adminToken}` });

  const load = useCallback(async () => {
    setLoading(true);
    setLoadError('');
    try {
      const res = await api.get('/admin/config/print-template', { headers: adminHeaders() });
      setRows(res.data);
    } catch (e: any) {
      setLoadError(e?.response?.data?.detail || 'Failed to load saved customisations. Showing built-in defaults.');
    }
    setLoading(false);
  }, [adminToken]);

  useEffect(() => { load(); }, [load]);

  // Resolve a base key to its storage key for the current mode
  const rk = (baseKey: string): string =>
    mode === 'export' ? (EXPORT_KEY_MAP[baseKey] ?? baseKey) : baseKey;

  // Get the base key from a resolved key
  const baseOf = (resolvedKey: string): string =>
    EXPORT_KEY_REVERSE[resolvedKey] ?? resolvedKey;

  // Get mode-appropriate default text for a base key
  const defaultFor = (baseKey: string, sharedFb: string): string => {
    const defs = mode === 'export' ? EXPORT_DEFAULTS : ARRIVAL_DEFAULTS;
    return defs[baseKey] ?? sharedFb;
  };

  // Get latest stored value for a resolved key, with fallback
  const storedVal = (resolvedKey: string, fallback: string): string => {
    const keyRows = rows
      .filter(r => r.field_key === resolvedKey)
      .sort((a, b) => b.effective_from.localeCompare(a.effective_from));
    return keyRows[0]?.field_value ?? fallback;
  };

  // Display value: mode-aware key + mode-aware default
  const v = (baseKey: string, sharedFb: string): string =>
    storedVal(rk(baseKey), defaultFor(baseKey, sharedFb));

  const getHistory = (resolvedKey: string): PtcRow[] =>
    rows.filter(r => r.field_key === resolvedKey)
      .sort((a, b) => b.effective_from.localeCompare(a.effective_from));

  // openEdit receives the resolved key (from ES fieldKey prop = rk(baseKey))
  const openEdit = (resolvedKey: string) => {
    const base = baseOf(resolvedKey);
    const fb = defaultFor(base, '');
    const current = storedVal(resolvedKey, fb);
    const storedLabel = rows.find(r => r.field_key === resolvedKey)?.field_label;
    const label = storedLabel ?? EXPORT_KEY_LABELS[resolvedKey] ?? resolvedKey;
    const isTemplate = PLACEHOLDER_BASE_KEYS.has(base);
    setEdit({ key: resolvedKey, label, currentValue: current, newValue: current, effectiveFrom: TODAY, isTemplate });
    setSaveMsg('');
    setShowHistory(false);
  };

  const handleSave = async () => {
    if (!edit) return;
    setSaving(true);
    setSaveMsg('');
    const label = edit.label || EXPORT_KEY_LABELS[edit.key] || edit.key;
    try {
      await api.post(
        '/admin/config/print-template',
        { field_key: edit.key, field_label: label, field_value: edit.newValue, effective_from: edit.effectiveFrom },
        { headers: adminHeaders() }
      );
      setSaveMsg('Saved. Effective from ' + edit.effectiveFrom + '.');
      await load();
    } catch (e: any) {
      setSaveMsg('Error: ' + (e.response?.data?.detail || 'Save failed'));
    }
    setSaving(false);
  };

  const sel = edit?.key;

  const SAMPLE = {
    paxName: 'SAMPLE PASSENGER NAME',
    osNo: 'OS-001/2025 (AIU)',
    osDate: '01-Jan-2025',
    passportNo: 'A1234567',
    flightNo: 'AI-123',
  };

  if (loading) return <div className="p-8 text-center text-slate-400 text-xs">Loading template...</div>;
  // loadError is non-fatal — show a banner but still render the editor with defaults

  return (
    <div className="flex flex-col gap-3 min-h-[600px]">
      {loadError && (
        <div className="px-3 py-2 bg-amber-50 border border-amber-300 rounded text-xs text-amber-800 font-medium">
          ⚠ {loadError}
        </div>
      )}
    <div className="flex gap-4 flex-1">

      {/* ── OS Shell (left / main) ───────────────────────────────────────── */}
      <div className={`flex-1 min-w-0 overflow-y-auto transition-[max-width] duration-200 ${edit ? 'max-w-[60%]' : 'max-w-full'}`}>

        {/* ── Mode switcher ────────────────────────────────────────────────── */}
        <div className="flex items-center gap-2 mb-3 p-2.5 bg-slate-50 border border-slate-200 rounded-lg">
          <span className="text-[10px] font-bold text-slate-500 uppercase tracking-wider shrink-0">Editing for:</span>
          <button
            onClick={() => { setMode('arrival'); setEdit(null); }}
            className={`flex items-center gap-1.5 px-3 py-1 rounded-full text-xs font-semibold transition-colors ${
              mode === 'arrival'
                ? 'bg-indigo-600 text-white shadow-sm'
                : 'bg-white border border-slate-200 text-slate-500 hover:bg-slate-100'
            }`}
          >
            <PlaneLanding size={12} /> Arrival Cases
          </button>
          <button
            onClick={() => { setMode('export'); setEdit(null); }}
            className={`flex items-center gap-1.5 px-3 py-1 rounded-full text-xs font-semibold transition-colors ${
              mode === 'export'
                ? 'bg-violet-600 text-white shadow-sm'
                : 'bg-white border border-slate-200 text-slate-500 hover:bg-slate-100'
            }`}
          >
            <PlaneTakeoff size={12} /> Export / Departure Cases
          </button>
          <span className="text-[9px] text-slate-400 leading-snug ml-1">
            {mode === 'arrival'
              ? 'Editing arrival (import) case print template.'
              : 'Highlighted fields below use separate export-specific values. Shared fields use the arrival value unless overridden here.'}
          </span>
        </div>

        <div className="text-[9px] text-slate-400 mb-2 text-center">
          Click any <span className="text-blue-500 font-medium">highlighted section</span> to edit it. Changes are versioned by effective date.
        </div>

        {/* ── PAGE 1 ──────────────────────────────────────────────────────── */}
        <div className="bg-white border border-slate-300 shadow-sm p-4 mb-4 font-serif text-[8.5pt] leading-snug" style={{ fontFamily: 'Arial, sans-serif' }}>
          {/* Header */}
          <div className="border-4 border-black flex items-center p-1 mb-3">
            <div className="w-12 h-12 bg-slate-100 flex items-center justify-center text-[8px] text-slate-400 mr-2 shrink-0">[Logo]</div>
            <div className="flex-1 text-center font-bold">
              <div>
                <ES fieldKey={rk('office_header_line1')} onEdit={openEdit} selected={sel === rk('office_header_line1')}>
                  {v('office_header_line1', 'Office of the Deputy / Asst. Commissioner of Customs')}
                </ES>
              </div>
              <div>
                <ES fieldKey={rk('office_header_line2')} onEdit={openEdit} selected={sel === rk('office_header_line2')}>
                  {v('office_header_line2', '(Airport), Anna International Airport, Chennai-600027')}
                </ES>
              </div>
            </div>
          </div>

          <div className="text-center font-bold uppercase text-sm mb-2">
            <ES fieldKey={rk('page1_title')} onEdit={openEdit} selected={sel === rk('page1_title')}>
              {v('page1_title', "Detention / Seizure of Passenger's Baggage")}
            </ES>
          </div>

          {/* Passenger info table */}
          <table className="w-full border-collapse border-4 border-black mb-2 text-[7.5pt]">
            <tbody>
              <tr>
                <td className="border border-black px-1 font-bold">O.S. No.</td>
                <td className="border-4 border-black px-1 italic text-slate-400">{SAMPLE.osNo}</td>
                <td className="border border-black px-1 font-bold">O.S. Date</td>
                <td className="border-4 border-black px-1 italic text-slate-400">{SAMPLE.osDate}</td>
                <td className="border border-black px-1 font-bold">Detention Date</td>
                <td className="border border-black px-1 italic text-slate-400">{SAMPLE.osDate}</td>
              </tr>
              <tr>
                <td className="border border-black px-1 font-bold" rowSpan={3}>Full Name of Passenger With Address in India</td>
                <td className="border-4 border-black px-1 italic text-slate-400" rowSpan={3} colSpan={3}>{SAMPLE.paxName}</td>
                <td className="border border-black px-1 font-bold">Passport No. &amp; Date</td>
                <td className="border border-black px-1 italic text-slate-400">{SAMPLE.passportNo}</td>
              </tr>
              <tr>
                <td className="border border-black px-1 font-bold">Flight No. &amp; Date</td>
                <td className="border border-black px-1 italic text-slate-400">{SAMPLE.flightNo}</td>
              </tr>
              <tr>
                <td className="border border-black px-1 font-bold">From / To</td>
                <td className="border border-black px-1 italic text-slate-400">
                  {mode === 'export' ? 'CHENNAI TO DUBAI' : 'DUBAI TO CHENNAI'}
                </td>
              </tr>
            </tbody>
          </table>

          {/* Inventory heading — export-specific */}
          <div className="text-center font-bold mb-1">
            <ES fieldKey={rk('inventory_heading')} onEdit={openEdit} selected={sel === rk('inventory_heading')}>
              {v('inventory_heading', 'INVENTORY OF THE GOODS IMPORTED')}
            </ES>
          </div>

          {/* Inventory table */}
          <table className="w-full border-collapse border-4 border-black mb-2 text-[7pt] text-center">
            <thead>
              <tr>
                <th className="border-2 border-black p-0.5">S.No.</th>
                <th className="border-2 border-black p-0.5 text-left">Description of Goods</th>
                <th className="border-2 border-black p-0.5">Qty.</th>
                <th className="border-2 border-black p-0.5">
                  <ES fieldKey={rk('col_fa_heading')} onEdit={openEdit} selected={sel === rk('col_fa_heading')}>
                    {v('col_fa_heading', 'Goods Allowed Free Under Rule 5 / Rule 13 of Baggage Rules, 1994')}
                  </ES>
                </th>
                <th className="border-2 border-black p-0.5">
                  <ES fieldKey={rk('col_duty_heading')} onEdit={openEdit} selected={sel === rk('col_duty_heading')}>
                    {v('col_duty_heading', 'Goods Passed On Duty')}
                  </ES>
                  <br/><span className="font-bold">Value (in Rs.)</span>
                </th>
                <th className="border-2 border-black p-0.5">
                  <ES fieldKey={rk('col_liable_heading')} onEdit={openEdit} selected={sel === rk('col_liable_heading')}>
                    {v('col_liable_heading', 'Goods Liable to Action Under FEMA / Foreign Trade Act, 1992 & Customs Act, 1962')}
                  </ES>
                  <br/><span className="font-bold">Total Value (in Rs.)</span>
                </th>
              </tr>
              <tr>
                <td className="border-2 border-black p-0.5 text-slate-300 italic">1</td>
                <td className="border-2 border-black p-0.5 text-left text-slate-300 italic">SAMPLE ITEM</td>
                <td className="border-2 border-black p-0.5 text-slate-300 italic">1 Nos.</td>
                <td className="border-2 border-black p-0.5 text-slate-300 italic">—</td>
                <td className="border-2 border-black p-0.5 text-slate-300 italic">—</td>
                <td className="border-2 border-black p-0.5 text-slate-300 italic">50,000</td>
              </tr>
            </thead>
          </table>

          {/* Summary table */}
          <table className="w-full border-collapse border-4 border-black mb-2 text-[7pt]">
            <tbody>
              <tr>
                <td className="border-2 border-black px-1 py-0.5">
                  Value of <ES fieldKey={rk('col_fa_heading')} onEdit={openEdit} selected={sel === rk('col_fa_heading')}>{v('col_fa_heading', 'Goods Allowed Free...')}</ES>
                </td>
                <td className="border-2 border-black px-1 py-0.5 text-right font-bold italic text-slate-400">Rs. 0/-</td>
              </tr>
              <tr>
                <td className="border-2 border-black px-1 py-0.5">
                  <ES fieldKey={rk('summary_duty_text')} onEdit={openEdit} selected={sel === rk('summary_duty_text')}>
                    {v('summary_duty_text', 'Value of Goods Charged to Duty Under Foreign Trade (D&R) Act, 1992 & Customs Act, 1962')}
                  </ES>
                </td>
                <td className="border-2 border-black px-1 py-0.5 text-right font-bold italic text-slate-400">Rs. 0</td>
              </tr>
              <tr>
                <td className="border-2 border-black px-1 py-0.5 font-bold">
                  <ES fieldKey={rk('summary_liable_text')} onEdit={openEdit} selected={sel === rk('summary_liable_text')}>
                    {v('summary_liable_text', 'Value of Goods Liable to Action under FEMA / Foreign Trade (D&R) Act, 1992 & Customs Act 1962')}
                  </ES>
                </td>
                <td className="border-2 border-black px-1 py-0.5 text-right font-bold italic text-slate-400">Rs. 50,000</td>
              </tr>
            </tbody>
          </table>

          <div className="flex justify-end font-bold text-[7.5pt] pr-4 mb-1">
            <ES fieldKey={rk('supdt_sig_title')} onEdit={openEdit} selected={sel === rk('supdt_sig_title')}>
              {v('supdt_sig_title', 'Supdt. of Customs')}
            </ES>
          </div>
        </div>

        {/* ── PAGE 2 ──────────────────────────────────────────────────────── */}
        <div className="bg-white border border-slate-300 shadow-sm p-4 font-serif text-[8pt] leading-snug" style={{ fontFamily: 'Arial, sans-serif' }}>
          <div className="text-center font-bold mb-1">
            <ES fieldKey={rk('p2_office_heading')} onEdit={openEdit} selected={sel === rk('p2_office_heading')}>
              {v('p2_office_heading', 'Office of the Deputy / Asst. Commissioner of Customs (Airport), Anna International airport, Chennai-600027.')}
            </ES>
          </div>

          {/* Waiver section */}
          <div className="text-center font-bold underline uppercase mb-1">
            <ES fieldKey={rk('p2_waiver_heading')} onEdit={openEdit} selected={sel === rk('p2_waiver_heading')}>
              {v('p2_waiver_heading', 'WAIVER OF SHOW CAUSE NOTICE')}
            </ES>
          </div>
          {/* waiver_text_1 is export-specific */}
          <p className="text-justify indent-4 mb-1">
            <ES fieldKey={rk('waiver_text_1')} onEdit={openEdit} selected={sel === rk('waiver_text_1')}>
              {v('waiver_text_1', 'The Charges have been orally communicated to me...')}
            </ES>
          </p>
          <p className="text-justify indent-4 mb-2">
            <ES fieldKey={rk('waiver_text_2')} onEdit={openEdit} selected={sel === rk('waiver_text_2')}>
              {v('waiver_text_2', 'I was present during the personal hearing conducted by the Deputy / Asst. Commissioner and I was heard.')}
            </ES>
          </p>

          {/* ORDER ORIGINAL */}
          <div className="text-center font-bold underline uppercase mb-1">ORDER (ORIGINAL)</div>
          <div className="mb-1 space-y-0.5 text-justify text-[7.5pt]">
            <p>
              <ES fieldKey={rk('nb1_text')} onEdit={openEdit} selected={sel === rk('nb1_text')}>
                {v('nb1_text', 'N.B: 1. This copy is granted free of charge for the private use of the person to whom it is issued.')}
              </ES>
            </p>
            <p>
              <ES fieldKey={rk('nb2_text')} onEdit={openEdit} selected={sel === rk('nb2_text')}>
                {v('nb2_text', '2. An Appeal against this Order shall lie before the Commissioner of Customs (Appeals), Custom House, Chennai-600 001 on payment of 7.5% of the duty demanded where duty or duty and penalty are in dispute, or penalty, where penalty alone is in dispute. The Appeal shall be filed within 60 days provided under Section 128 of the Customs Act, 1962 from the date of receipt of this Order.')}
              </ES>
            </p>
          </div>
          <p className="mb-1 font-bold">
            <ES fieldKey={rk('note_scn_waived')} onEdit={openEdit} selected={sel === rk('note_scn_waived')}>
              {v('note_scn_waived', 'Note: The issue of Show Cause Notice was waived at the instance of the Passenger.')}
            </ES>
          </p>

          {/* Legal paras — export-specific */}
          <p className="text-justify indent-4 mb-1 text-[7.5pt]">
            <ES fieldKey={rk('legal_para_1')} onEdit={openEdit} selected={sel === rk('legal_para_1')}>
              {v('legal_para_1', 'In terms of Foreign Trade Policy...')}
            </ES>
          </p>
          <p className="text-justify indent-4 mb-2 text-[7.5pt]">
            <ES fieldKey={rk('legal_para_2')} onEdit={openEdit} selected={sel === rk('legal_para_2')}>
              {v('legal_para_2', 'Import/Export of goods non-declared...')}
            </ES>
          </p>

          {/* Record heading */}
          <div className="text-center font-bold underline uppercase mb-1">
            <ES fieldKey={rk('record_heading')} onEdit={openEdit} selected={sel === rk('record_heading')}>
              {v('record_heading', 'RECORD OF PERSONAL HEARING & FINDINGS')}
            </ES>
          </div>
          <p className="text-justify indent-4 mb-2 italic text-slate-400 text-[7.5pt]">[AC/DC remarks go here]</p>

          {/* ORDER heading */}
          <div className="text-center font-bold underline uppercase mb-1">
            <ES fieldKey={rk('order_heading')} onEdit={openEdit} selected={sel === rk('order_heading')}>
              {v('order_heading', 'ORDER')}
            </ES>
          </div>

          {/* ORDER paragraphs — section refs differ between arrival/export */}
          <div className="text-[7.5pt] text-justify space-y-1 mb-2">
            <p className="indent-4">
              <ES fieldKey={rk('order_para_rf')} onEdit={openEdit} selected={sel === rk('order_para_rf')}>
                <TemplateText text={v('order_para_rf', ARRIVAL_DEFAULTS.order_para_rf ?? '')} />
              </ES>
            </p>

            {/* Re-export paragraph — arrival only */}
            {mode === 'arrival' ? (
              <p className="indent-4">
                <ES fieldKey="order_para_ref" onEdit={openEdit} selected={sel === 'order_para_ref'}>
                  <TemplateText text={v('order_para_ref', ARRIVAL_DEFAULTS.order_para_ref ?? '')} />
                </ES>
              </p>
            ) : (
              <p className="indent-4 text-[7pt] text-slate-400 italic border border-dashed border-violet-200 rounded px-2 py-0.5">
                Re-export / reship option (order_para_ref) — not applicable for export / departure cases; suppressed in print
              </p>
            )}

            <p className="indent-4">
              <ES fieldKey={rk('order_para_abs_conf')} onEdit={openEdit} selected={sel === rk('order_para_abs_conf')}>
                <TemplateText text={v('order_para_abs_conf', ARRIVAL_DEFAULTS.order_para_abs_conf ?? '')} />
              </ES>
            </p>
            <p className="indent-4">
              <ES fieldKey={rk('order_para_pp')} onEdit={openEdit} selected={sel === rk('order_para_pp')}>
                <TemplateText text={v('order_para_pp', ARRIVAL_DEFAULTS.order_para_pp ?? '')} />
              </ES>
            </p>
          </div>

          {/* Deputy sig */}
          <div className="flex justify-end font-bold text-[7.5pt] mb-2">
            <ES fieldKey={rk('deputy_sig_title')} onEdit={openEdit} selected={sel === rk('deputy_sig_title')}>
              {v('deputy_sig_title', 'Deputy / Asst. Commissioner of Customs (Airport)')}
            </ES>
          </div>

          {/* Bottom N.B. */}
          <div className="text-[7.5pt] space-y-0 mb-2">
            <p>
              <ES fieldKey={rk('bottom_nb1')} onEdit={openEdit} selected={sel === rk('bottom_nb1')}>
                {v('bottom_nb1', 'N.B: 1. Perishables will be disposed off within seven days from the date of detention.')}
              </ES>
            </p>
            {/* bottom_nb2 (re-export note) — arrival only */}
            {mode === 'arrival' ? (
              <p>
                <ES fieldKey="bottom_nb2" onEdit={openEdit} selected={sel === 'bottom_nb2'}>
                  {v('bottom_nb2', '2. Where re-export is permitted, the passenger is advised to intimate the date of departure of flight atleast 48 hours in advance.')}
                </ES>
              </p>
            ) : (
              <p className="text-[7pt] text-slate-400 italic border border-dashed border-violet-200 rounded px-2 py-0.5">
                NB 2 (re-export note) — suppressed for export / departure cases
              </p>
            )}
            <p>
              <ES fieldKey={rk('bottom_nb3')} onEdit={openEdit} selected={sel === rk('bottom_nb3')}>
                {v('bottom_nb3', '3. Warehouse rent and Handling Charges are chargeable for the goods detained.')}
              </ES>
            </p>
          </div>

          <div className="flex justify-end font-bold text-[7.5pt]">
            <ES fieldKey={rk('received_order_text')} onEdit={openEdit} selected={sel === rk('received_order_text')}>
              {v('received_order_text', 'Received the Order-in-Original')}
            </ES>
          </div>
        </div>
      </div>

      {/* ── Edit Panel (right) ────────────────────────────────────────────── */}
      {edit && (
        <div className="w-[40%] min-w-[280px] flex flex-col bg-white border border-slate-200 rounded-xl shadow-lg overflow-hidden sticky top-0 max-h-[90vh]">
          {/* Header */}
          <div className={`flex items-center justify-between px-4 py-3 border-b ${
            mode === 'export' ? 'bg-violet-50 border-violet-100' : 'bg-slate-50 border-slate-100'
          }`}>
            <div>
              <p className={`text-[10px] font-semibold uppercase tracking-wider ${
                mode === 'export' ? 'text-violet-600' : 'text-blue-600'
              }`}>
                {mode === 'arrival' ? 'Arrival Cases' : 'Export / Departure Cases'}
              </p>
              <p className="text-xs font-bold text-slate-800">{edit.label}</p>
              <p className="text-[9px] text-slate-400 font-mono">{edit.key}</p>
            </div>
            <button onClick={() => setEdit(null)} className="text-slate-400 hover:text-slate-600">
              <X size={16}/>
            </button>
          </div>

          <div className="flex-1 overflow-y-auto p-4 space-y-4">
            {edit.isTemplate && (
              <div className="bg-amber-50 border border-amber-200 rounded-lg p-2 text-[10px] text-amber-700">
                <p className="font-semibold mb-1">This is a template paragraph</p>
                <p>Parts like <code className="bg-amber-100 px-1 rounded">{'{rf_slnos_text}'}</code> are dynamic placeholders filled at print time. Do not remove them — only edit the surrounding legal text.</p>
              </div>
            )}

            <div>
              <label className="block text-[10px] font-semibold text-slate-600 mb-1">New Text</label>
              <textarea
                className="w-full border border-slate-200 rounded-lg px-3 py-2 text-xs focus:outline-none focus:ring-2 focus:ring-blue-400 resize-none"
                rows={edit.isTemplate ? 8 : 5}
                value={edit.newValue}
                onChange={e => setEdit(prev => prev ? { ...prev, newValue: e.target.value } : null)}
              />
              {edit.isTemplate && (
                <div className="mt-1 text-[10px]">
                  <TemplateText text={edit.newValue} />
                </div>
              )}
            </div>

            <div>
              <label className="block text-[10px] font-semibold text-slate-600 mb-1">
                Effective From
                <span className="text-slate-400 font-normal ml-1">— OS cases on or after this date will show this new text</span>
              </label>
              <input
                type="date"
                value={edit.effectiveFrom}
                onChange={e => setEdit(prev => prev ? { ...prev, effectiveFrom: e.target.value } : null)}
                className="w-full border border-slate-200 rounded-lg px-3 py-2 text-xs focus:outline-none focus:ring-2 focus:ring-blue-400"
              />
              <p className="text-[9px] text-slate-400 mt-0.5">OS cases before this date will continue to show the previous version.</p>
            </div>

            <button
              onClick={handleSave}
              disabled={saving || !edit.newValue.trim()}
              className="w-full flex items-center justify-center gap-2 py-2 rounded-lg bg-blue-600 text-white text-xs font-semibold hover:bg-blue-700 disabled:opacity-50 transition"
            >
              <Save size={13}/> {saving ? 'Saving...' : 'Save New Version'}
            </button>

            {saveMsg && (
              <p className={`text-[10px] text-center ${saveMsg.startsWith('Error') ? 'text-red-600' : 'text-emerald-600'}`}>
                {saveMsg}
              </p>
            )}

            <div className="border-t border-slate-100 pt-3">
              <button
                className="flex items-center gap-1 text-[10px] font-semibold text-slate-500 hover:text-slate-700 w-full"
                onClick={() => setShowHistory(h => !h)}
              >
                <Clock size={11}/>
                Version History ({getHistory(edit.key).length} version{getHistory(edit.key).length !== 1 ? 's' : ''})
                {showHistory ? <ChevronUp size={11} className="ml-auto"/> : <ChevronDown size={11} className="ml-auto"/>}
              </button>
              {showHistory && (
                <div className="mt-2 space-y-2 max-h-48 overflow-y-auto">
                  {getHistory(edit.key).length === 0 && (
                    <p className="text-[10px] text-slate-400 italic">No versions saved yet (using built-in default text).</p>
                  )}
                  {getHistory(edit.key).map((row, i) => (
                    <div key={row.id} className={`rounded-lg p-2 text-[9px] border ${i === 0 ? 'border-blue-200 bg-blue-50' : 'border-slate-100 bg-slate-50'}`}>
                      <div className="flex items-center justify-between mb-0.5">
                        <span className="font-semibold text-slate-600">From: {row.effective_from}</span>
                        {i === 0 && <span className="text-blue-500 font-bold text-[8px]">LATEST</span>}
                        <button
                          className="text-slate-400 hover:text-slate-600 text-[8px] underline"
                          onClick={() => setEdit(prev => prev ? { ...prev, newValue: row.field_value } : null)}
                        >
                          use this
                        </button>
                      </div>
                      <p className="text-slate-500 line-clamp-3 leading-snug">{row.field_value}</p>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
    </div>
  );
}
