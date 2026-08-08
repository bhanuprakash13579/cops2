/**
 * Rates & Formulas configuration page.
 * Accessible from the Revenue module dashboard — NOT shown in the normal entry workflow.
 *
 * Two sections:
 *  1. Current Tariff Rates — view rates + "Set New Rates" to append a new tariff row
 *  2. Formula Rules — per-column formula expressions; all columns editable
 */
import { useState, useEffect, useCallback } from 'react';
import {
  ArrowLeft, Plus, Trash2, ChevronDown, ChevronRight,
  AlertTriangle, CheckCircle, Info, GripVertical,
} from 'lucide-react';
import api from '@/lib/api';
import {
  DrFormulaRule, DrTariff, FORMULA_VARS, evalFormula, buildFormulaVars, validateExpression,
} from './revenueCalc';

// All computable columns, in display order
const COMPUTABLE_COLS: { key: string; label: string }[] = [
  { key: 'baggage_duty',    label: 'Baggage Duty' },
  { key: 'liquor_duty',     label: 'Liquor Duty' },
  { key: 'cigarette_duty',  label: 'Cigarettes Duty' },
  { key: 'sw_sc',           label: 'SW SC on Bagg/Lqr/Cig' },
  { key: 'gold_duty_bcd',   label: 'Gold Duty (BCD)' },
  { key: 'gold_duty_cons',  label: 'Gold Duty (Concessional BCD)' },
  { key: 'silver_duty_cons','label': 'Silver Duty (Concessional)' },
  { key: 'sws_on_gold',     label: 'SWS on Gold' },
  { key: 'aidc_gold_silver',label: 'AIDC on Gold/Silver' },
  { key: 'sws_on_silver',   label: 'SWS on Silver' },
  { key: 'aidc_on_liquor',  label: 'AIDC on Liquor' },
  { key: 'redemption_fine', label: 'Redemption Fine' },
  { key: 'reexport_fine',   label: 'Re-Export Fine' },
  { key: 'personal_penalty','label': 'Personal Penalty' },
  { key: 'other_charges',   label: 'Other Charges' },
  { key: 'fuel_duty',       label: 'Fuel Duty' },
];

const COL_LABEL: Record<string, string> = Object.fromEntries(
  COMPUTABLE_COLS.map(c => [c.key, c.label])
);

// ── Confirmation modal ────────────────────────────────────────────────────────
interface ConfirmProps {
  title: string;
  lines: string[];
  safetyNote: string;
  onConfirm: () => void;
  onCancel: () => void;
  confirmLabel?: string;
  danger?: boolean;
}
function ConfirmModal({ title, lines, safetyNote, onConfirm, onCancel, confirmLabel = 'Confirm', danger = false }: ConfirmProps) {
  return (
    <div className="fixed inset-0 bg-black/50 z-50 flex items-center justify-center p-4">
      <div className="bg-white rounded-2xl shadow-2xl max-w-lg w-full p-6">
        <h3 className="text-base font-bold text-slate-800 mb-3">{title}</h3>
        {lines.map((line, i) => (
          <p key={i} className="text-sm text-slate-700 mb-1 font-mono bg-slate-50 rounded px-2 py-1">{line}</p>
        ))}
        <div className="mt-4 flex items-start gap-2 bg-amber-50 border border-amber-200 rounded-lg px-3 py-2">
          <AlertTriangle size={14} className="text-amber-600 shrink-0 mt-0.5" />
          <p className="text-xs text-amber-800">{safetyNote}</p>
        </div>
        <div className="flex gap-3 justify-end mt-5">
          <button onClick={onCancel}
            className="px-4 py-2 text-sm font-medium text-slate-700 bg-slate-100 hover:bg-slate-200 rounded-lg">
            Cancel
          </button>
          <button onClick={onConfirm}
            className={`px-4 py-2 text-sm font-bold text-white rounded-lg ${danger ? 'bg-red-600 hover:bg-red-700' : 'bg-teal-600 hover:bg-teal-700'}`}>
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}

// ── Rate field display ────────────────────────────────────────────────────────
const RATE_GROUPS = [
  {
    label: 'Baggage',
    fields: [{ key: 'baggage_rate', label: 'Baggage Rate' }],
  },
  {
    label: 'Liquor',
    fields: [
      { key: 'liquor_duty_rate', label: 'Liquor Duty Rate' },
      { key: 'aidc_liquor_rate', label: 'AIDC on Liquor Rate' },
    ],
  },
  {
    label: 'Gold (Standard BCD)',
    fields: [
      { key: 'gold_bcd_rate', label: 'Gold BCD Rate' },
      { key: 'aidc_gold_rate', label: 'AIDC on Gold/Silver Rate' },
    ],
  },
  {
    label: 'Gold (Concessional)',
    fields: [
      { key: 'gold_cons_bcd_rate', label: 'Gold Concessional BCD Rate' },
      { key: 'aidc_gold_cons_rate', label: 'AIDC on Gold(C) Rate' },
    ],
  },
  {
    label: 'Silver (Concessional)',
    fields: [
      { key: 'silver_bcd_rate', label: 'Silver BCD Rate' },
      { key: 'aidc_silver_rate', label: 'AIDC on Silver Rate' },
      { key: 'silver_cons_rate', label: 'Silver Concessional Rate' },
      { key: 'aidc_silver_cons_rate', label: 'AIDC on Silver(C) Rate' },
    ],
  },
];

interface Props {
  onBack: () => void;
  onRulesChanged: (rules: DrFormulaRule[]) => void;
}

export default function FormulaRulesPage({ onBack, onRulesChanged }: Props) {
  const [tariff, setTariff] = useState<DrTariff | null>(null);
  const [rules, setRules] = useState<DrFormulaRule[]>([]);
  const [loading, setLoading] = useState(true);

  // ── Rates section state ──────────────────────────────────────────────────
  const [showRateHistory, setShowRateHistory] = useState(false);
  const [rateHistory, setRateHistory] = useState<DrTariff[]>([]);
  const [showNewRateForm, setShowNewRateForm] = useState(false);
  const [newRate, setNewRate] = useState<Partial<DrTariff>>({});
  const [rateConfirm, setRateConfirm] = useState<null | { oldT: DrTariff; newT: Partial<DrTariff> }>(null);
  const [savingRate, setSavingRate] = useState(false);

  // ── Formula section state ────────────────────────────────────────────────
  const [editingId, setEditingId] = useState<number | 'new' | null>(null);
  const [editDraft, setEditDraft] = useState<Partial<DrFormulaRule>>({});
  const [formulaConfirm, setFormulaConfirm] = useState<null | { old: DrFormulaRule | null; draft: Partial<DrFormulaRule> }>(null);
  const [deleteConfirm, setDeleteConfirm] = useState<DrFormulaRule | null>(null);
  const [showVarRef, setShowVarRef] = useState(false);
  const [exprPreview, setExprPreview] = useState<string | null>(null);

  useEffect(() => {
    Promise.all([
      api.get('/dcr/tariffs/current'),
      api.get('/dcr/formula-rules'),
      api.get('/dcr/tariffs'),
    ]).then(([tRes, rRes, hRes]) => {
      setTariff(tRes.data);
      setRules(rRes.data);
      setRateHistory(hRes.data);
      onRulesChanged(rRes.data);
    }).catch(() => {}).finally(() => setLoading(false));
  }, []); // eslint-disable-line

  const reloadRules = useCallback(() => {
    api.get('/dcr/formula-rules').then(r => {
      setRules(r.data);
      onRulesChanged(r.data);
    }).catch(() => {});
  }, [onRulesChanged]);

  // ── Rate helpers ──────────────────────────────────────────────────────────

  const startNewRate = () => {
    if (!tariff) return;
    const draft: Partial<DrTariff> = { ...tariff, id: undefined, label: '', effective_from: new Date().toISOString().split('T')[0] };
    setNewRate(draft);
    setShowNewRateForm(true);
  };

  const submitNewRate = () => {
    if (!tariff) return;
    setRateConfirm({ oldT: tariff, newT: newRate });
  };

  const confirmRate = async () => {
    setSavingRate(true);
    try {
      const payload = { ...newRate, effective_from: newRate.effective_from };
      await api.post('/dcr/tariffs', payload);
      const [tRes, hRes] = await Promise.all([
        api.get('/dcr/tariffs/current'),
        api.get('/dcr/tariffs'),
      ]);
      setTariff(tRes.data);
      setRateHistory(hRes.data);
      setShowNewRateForm(false);
      setRateConfirm(null);
    } catch { /* silent */ }
    finally { setSavingRate(false); }
  };

  // ── Formula helpers ───────────────────────────────────────────────────────

  const startEdit = (rule: DrFormulaRule) => {
    setEditingId(rule.id);
    setEditDraft({ ...rule });
    setExprPreview(null);
  };

  const startAdd = () => {
    setEditingId('new');
    setEditDraft({
      sort_order: rules.length,
      target_column: COMPUTABLE_COLS[0].key,
      column_label: COMPUTABLE_COLS[0].label,
      condition_type: 'all',
      condition_items: '',
      expression: '',
      is_active: true,
      notes: '',
    });
    setExprPreview(null);
  };

  const previewExpr = (expr: string) => {
    if (!tariff || !expr.trim()) { setExprPreview(null); return; }
    const vars = buildFormulaVars(tariff, 10000, 50);
    const err = validateExpression(expr, vars);
    if (err) { setExprPreview(`Error: ${err}`); return; }
    const result = evalFormula(expr, vars);
    setExprPreview(`Preview (value=10000, weight=50g): ₹${result.toLocaleString('en-IN', { minimumFractionDigits: 2 })}`);
  };

  const requestSave = () => {
    if (!editDraft.target_column) return;
    const old = rules.find(r => r.id === editingId) ?? null;
    setFormulaConfirm({ old, draft: editDraft });
  };

  const confirmSave = async () => {
    if (!editDraft.target_column) return;
    try {
      if (editingId === 'new') {
        await api.post('/dcr/formula-rules', editDraft);
      } else {
        await api.put(`/dcr/formula-rules/${editingId}`, editDraft);
      }
      reloadRules();
    } catch { /* silent */ }
    setFormulaConfirm(null);
    setEditingId(null);
  };

  const confirmDelete = async (rule: DrFormulaRule) => {
    try {
      await api.delete(`/dcr/formula-rules/${rule.id}`);
      reloadRules();
    } catch { /* silent */ }
    setDeleteConfirm(null);
  };

  const moveRule = async (rule: DrFormulaRule, dir: 'up' | 'down') => {
    const idx = rules.findIndex(r => r.id === rule.id);
    const swapIdx = dir === 'up' ? idx - 1 : idx + 1;
    if (swapIdx < 0 || swapIdx >= rules.length) return;
    const next = [...rules];
    [next[idx], next[swapIdx]] = [next[swapIdx], next[idx]];
    const order = next.map(r => r.id);
    try {
      await api.post('/dcr/formula-rules/reorder', order);
      setRules(next.map((r, i) => ({ ...r, sort_order: i })));
      onRulesChanged(next.map((r, i) => ({ ...r, sort_order: i })));
    } catch { /* silent */ }
  };

  const conditionLabel = (rule: DrFormulaRule) => {
    if (rule.condition_type === 'all') return 'All items';
    if (rule.condition_type === 'only') return `Only: ${rule.condition_items}`;
    return `All except: ${rule.condition_items}`;
  };

  if (loading) return (
    <div className="min-h-screen bg-slate-50 flex items-center justify-center">
      <p className="text-sm text-slate-400 animate-pulse">Loading config…</p>
    </div>
  );

  return (
    <div className="min-h-screen bg-slate-50">
      {/* Header */}
      <div className="flex items-center gap-3 px-4 py-3 bg-slate-800 text-white sticky top-0 z-10">
        <button onClick={onBack} className="text-slate-300 hover:text-white"><ArrowLeft size={18} /></button>
        <span className="font-bold text-base">⚙ Rates &amp; Formulas</span>
        <span className="text-slate-400 text-xs ml-2">— changes here only affect new entries. Historical data is always protected.</span>
      </div>

      <div className="max-w-4xl mx-auto p-6 space-y-8">

        {/* ── Section 1: Current Tariff Rates ─────────────────────────────── */}
        <section className="bg-white rounded-2xl border border-slate-200 overflow-hidden">
          <div className="flex items-center justify-between px-5 py-4 border-b border-slate-100">
            <div>
              <h2 className="font-bold text-slate-800">Current Tariff Rates</h2>
              <p className="text-xs text-slate-500 mt-0.5">
                {tariff
                  ? `Active since: ${tariff.effective_from}${tariff.label ? ` — ${tariff.label}` : ''}`
                  : 'No tariff defined'}
              </p>
            </div>
            <button
              onClick={startNewRate}
              className="px-4 py-2 text-sm font-semibold bg-teal-600 hover:bg-teal-700 text-white rounded-xl"
            >
              Set New Rates
            </button>
          </div>

          {tariff && (
            <div className="p-5 space-y-5">
              {RATE_GROUPS.map(group => (
                <div key={group.label}>
                  <p className="text-[10px] font-bold text-slate-400 uppercase tracking-widest mb-2">{group.label}</p>
                  <div className="grid grid-cols-2 sm:grid-cols-3 gap-2">
                    {group.fields.map(f => (
                      <div key={f.key} className="bg-slate-50 rounded-xl px-3 py-2">
                        <p className="text-[10px] text-slate-500 mb-0.5">{f.label}</p>
                        <p className="text-sm font-bold text-slate-800">
                          {((tariff[f.key as keyof DrTariff] as number) * 100).toFixed(0)}%
                        </p>
                      </div>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          )}

          {/* New rate form */}
          {showNewRateForm && tariff && (
            <div className="border-t border-slate-200 p-5 bg-teal-50">
              <h3 className="font-semibold text-slate-800 mb-4 text-sm">Set New Rates (effective from today)</h3>
              <div className="grid grid-cols-2 gap-3 mb-4">
                <div className="col-span-2">
                  <label className="text-xs text-slate-500">Effective From</label>
                  <input type="date" value={String(newRate.effective_from ?? '')}
                    onChange={e => setNewRate(p => ({ ...p, effective_from: e.target.value as unknown as string }))}
                    className="mt-1 w-full text-sm border border-slate-300 rounded-lg px-3 py-1.5 focus:outline-none focus:ring-2 focus:ring-teal-400" />
                </div>
                <div className="col-span-2">
                  <label className="text-xs text-slate-500">Label (optional)</label>
                  <input type="text" value={String(newRate.label ?? '')} placeholder="e.g. Budget 2025-26"
                    onChange={e => setNewRate(p => ({ ...p, label: e.target.value }))}
                    className="mt-1 w-full text-sm border border-slate-300 rounded-lg px-3 py-1.5 focus:outline-none focus:ring-2 focus:ring-teal-400" />
                </div>
                {RATE_GROUPS.flatMap(g => g.fields).map(f => (
                  <div key={f.key}>
                    <label className="text-xs text-slate-500">{f.label} (%)</label>
                    <input type="number" step="0.01" min="0" max="999"
                      value={((newRate[f.key as keyof DrTariff] as number ?? 0) * 100).toFixed(2)}
                      onChange={e => setNewRate(p => ({ ...p, [f.key]: Number(e.target.value) / 100 }))}
                      className="mt-1 w-full text-sm border border-slate-300 rounded-lg px-3 py-1.5 focus:outline-none focus:ring-2 focus:ring-teal-400" />
                  </div>
                ))}
              </div>
              <div className="flex gap-3">
                <button onClick={() => setShowNewRateForm(false)}
                  className="px-4 py-2 text-sm text-slate-600 bg-slate-100 hover:bg-slate-200 rounded-lg">
                  Cancel
                </button>
                <button onClick={submitNewRate}
                  className="px-4 py-2 text-sm font-semibold bg-teal-600 hover:bg-teal-700 text-white rounded-lg">
                  Review &amp; Save
                </button>
              </div>
            </div>
          )}

          {/* Rate history */}
          <div className="border-t border-slate-100">
            <button
              onClick={() => setShowRateHistory(s => !s)}
              className="flex items-center gap-2 px-5 py-3 text-xs text-slate-500 hover:text-slate-700 font-medium"
            >
              {showRateHistory ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
              Rate History ({rateHistory.length} entries)
            </button>
            {showRateHistory && (
              <div className="px-5 pb-4 space-y-1.5">
                {rateHistory.map(t => (
                  <div key={t.id} className={`text-xs rounded-lg px-3 py-2 ${t.id === tariff?.id ? 'bg-teal-50 border border-teal-200' : 'bg-slate-50'}`}>
                    <span className="font-semibold">{t.effective_from}</span>
                    {t.label && <span className="text-slate-500"> — {t.label}</span>}
                    {t.id === tariff?.id && <span className="ml-2 text-teal-700 font-bold">● Current</span>}
                    <span className="ml-2 text-slate-400">Baggage: {(t.baggage_rate * 100).toFixed(0)}%, Gold BCD: {(t.gold_bcd_rate * 100).toFixed(0)}%</span>
                  </div>
                ))}
              </div>
            )}
          </div>
        </section>

        {/* ── Section 2: Formula Rules ─────────────────────────────────────── */}
        <section className="bg-white rounded-2xl border border-slate-200 overflow-hidden">
          <div className="flex items-center justify-between px-5 py-4 border-b border-slate-100">
            <div>
              <h2 className="font-bold text-slate-800">Formula Rules</h2>
              <p className="text-xs text-slate-500 mt-0.5">
                Rules evaluated top-to-bottom. First matching rule for a column wins.
                Empty formula = manual entry.
              </p>
            </div>
            <button onClick={startAdd}
              className="flex items-center gap-2 px-4 py-2 text-sm font-semibold bg-slate-700 hover:bg-slate-800 text-white rounded-xl">
              <Plus size={14} /> Add Rule
            </button>
          </div>

          {/* Variable reference */}
          <div className="border-b border-slate-100">
            <button onClick={() => setShowVarRef(s => !s)}
              className="flex items-center gap-2 px-5 py-2.5 text-xs text-blue-700 hover:text-blue-900 font-medium">
              {showVarRef ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
              <Info size={12} /> Expression Variables Reference
            </button>
            {showVarRef && (
              <div className="px-5 pb-4">
                <div className="grid grid-cols-2 sm:grid-cols-3 gap-1.5">
                  {FORMULA_VARS.map(v => (
                    <div key={v.name} className="flex items-center gap-2 bg-slate-50 rounded-lg px-2 py-1.5">
                      <code className="text-xs font-bold text-blue-700 bg-blue-50 px-1.5 py-0.5 rounded">{v.name}</code>
                      <span className="text-[10px] text-slate-500">{v.label}</span>
                    </div>
                  ))}
                </div>
                <p className="text-[10px] text-slate-400 mt-2">
                  Operators: + − × ÷ and parentheses. Example: <code className="bg-slate-100 px-1 rounded">value * baggage_rate</code>
                </p>
              </div>
            )}
          </div>

          {/* New rule inline form */}
          {editingId === 'new' && (
            <RuleEditForm
              draft={editDraft}
              onChange={setEditDraft}
              onPreview={previewExpr}
              preview={exprPreview}
              onSave={requestSave}
              onCancel={() => setEditingId(null)}
              tariff={tariff}
            />
          )}

          {/* Rule list */}
          {rules.length === 0 ? (
            <p className="text-sm text-slate-400 text-center py-12">No formula rules yet. Click "Add Rule" to start.</p>
          ) : (
            <div className="divide-y divide-slate-100">
              {rules.map((rule, idx) => (
                <div key={rule.id}>
                  {editingId === rule.id ? (
                    <RuleEditForm
                      draft={editDraft}
                      onChange={setEditDraft}
                      onPreview={previewExpr}
                      preview={exprPreview}
                      onSave={requestSave}
                      onCancel={() => setEditingId(null)}
                      tariff={tariff}
                    />
                  ) : (
                    <div className={`flex items-start gap-3 px-4 py-3 hover:bg-slate-50 group ${!rule.is_active ? 'opacity-50' : ''}`}>
                      {/* Reorder */}
                      <div className="flex flex-col gap-0.5 mt-1 shrink-0">
                        <button onClick={() => moveRule(rule, 'up')} disabled={idx === 0}
                          className="text-slate-300 hover:text-slate-600 disabled:opacity-20 text-[10px] leading-none">▲</button>
                        <button onClick={() => moveRule(rule, 'down')} disabled={idx === rules.length - 1}
                          className="text-slate-300 hover:text-slate-600 disabled:opacity-20 text-[10px] leading-none">▼</button>
                      </div>
                      <GripVertical size={14} className="text-slate-200 mt-1 shrink-0" />

                      {/* Rule details */}
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2 flex-wrap">
                          <span className="text-xs font-bold text-slate-800">
                            {rule.column_label || COL_LABEL[rule.target_column] || rule.target_column}
                          </span>
                          <span className="text-[10px] text-slate-400 bg-slate-100 rounded px-1.5 py-0.5">{rule.target_column}</span>
                          <span className="text-[10px] text-blue-600 bg-blue-50 rounded px-1.5 py-0.5">{conditionLabel(rule)}</span>
                          {!rule.is_active && <span className="text-[10px] text-slate-400 italic">inactive</span>}
                        </div>
                        <div className="mt-1">
                          {rule.expression.trim() ? (
                            <code className="text-xs text-emerald-700 bg-emerald-50 px-2 py-0.5 rounded">{rule.expression}</code>
                          ) : (
                            <span className="text-[10px] text-slate-400 italic">manual entry (no formula)</span>
                          )}
                        </div>
                        {rule.notes && <p className="text-[10px] text-slate-400 mt-0.5">{rule.notes}</p>}
                      </div>

                      {/* Actions */}
                      <div className="flex items-center gap-1.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
                        <button onClick={() => startEdit(rule)}
                          className="text-xs px-2 py-1 bg-slate-100 hover:bg-slate-200 text-slate-700 rounded-lg">
                          Edit
                        </button>
                        <button onClick={() => setDeleteConfirm(rule)}
                          className="text-red-400 hover:text-red-600 p-1">
                          <Trash2 size={13} />
                        </button>
                      </div>
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </section>
      </div>

      {/* ── Confirmation modals ──────────────────────────────────────────────── */}

      {rateConfirm && tariff && (
        <ConfirmModal
          title="Set New Tariff Rates"
          lines={
            RATE_GROUPS.flatMap(g => g.fields)
              .filter(f => (rateConfirm.newT[f.key as keyof DrTariff] as number) !== (rateConfirm.oldT[f.key as keyof DrTariff] as number))
              .map(f => {
                const oldPct = ((rateConfirm.oldT[f.key as keyof DrTariff] as number) * 100).toFixed(2);
                const newPct = ((rateConfirm.newT[f.key as keyof DrTariff] as number ?? 0) * 100).toFixed(2);
                return `${f.label}: ${oldPct}% → ${newPct}%`;
              })
          }
          safetyNote="Already-entered entries are NOT recomputed. Only new entries you create after this will use the updated rates."
          onConfirm={confirmRate}
          onCancel={() => setRateConfirm(null)}
          confirmLabel={savingRate ? 'Saving…' : 'Save New Rates'}
        />
      )}

      {formulaConfirm && (
        <ConfirmModal
          title={formulaConfirm.old ? 'Update Formula Rule' : 'Add Formula Rule'}
          lines={[
            `Column: ${COL_LABEL[formulaConfirm.draft.target_column ?? ''] ?? formulaConfirm.draft.target_column ?? ''}`,
            `Condition: ${formulaConfirm.draft.condition_type === 'all' ? 'All items' : formulaConfirm.draft.condition_type === 'only' ? `Only: ${formulaConfirm.draft.condition_items}` : `Except: ${formulaConfirm.draft.condition_items}`}`,
            ...(formulaConfirm.old?.expression !== formulaConfirm.draft.expression
              ? [`Formula: ${formulaConfirm.old?.expression || '(manual)'} → ${formulaConfirm.draft.expression || '(manual)'}`]
              : [`Formula: ${formulaConfirm.draft.expression || '(manual)'}`]),
          ]}
          safetyNote="Already-entered entries are NOT affected. Only new entries created after this will use the updated formula."
          onConfirm={confirmSave}
          onCancel={() => setFormulaConfirm(null)}
          confirmLabel="Save Formula"
        />
      )}

      {deleteConfirm && (
        <ConfirmModal
          title="Delete Formula Rule"
          lines={[
            `Column: ${COL_LABEL[deleteConfirm.target_column] ?? deleteConfirm.target_column}`,
            `Expression: ${deleteConfirm.expression || '(manual)'}`,
          ]}
          safetyNote="This removes the auto-computation rule for this column. The column becomes manual for the affected items. Historical data is NOT changed."
          onConfirm={() => confirmDelete(deleteConfirm)}
          onCancel={() => setDeleteConfirm(null)}
          confirmLabel="Delete Rule"
          danger
        />
      )}
    </div>
  );
}

// ── Inline rule edit form ────────────────────────────────────────────────────
interface EditFormProps {
  draft: Partial<DrFormulaRule>;
  onChange: (d: Partial<DrFormulaRule>) => void;
  onPreview: (expr: string) => void;
  preview: string | null;
  onSave: () => void;
  onCancel: () => void;
  tariff: DrTariff | null;
}
function RuleEditForm({ draft, onChange, onPreview, preview, onSave, onCancel }: EditFormProps) {
  const exprErr = draft.expression ? validateExpression(draft.expression) : null;

  return (
    <div className="px-5 py-4 bg-blue-50 border-b border-blue-200 space-y-3">
      <div className="grid grid-cols-2 gap-3">
        {/* Column */}
        <div>
          <label className="text-xs text-slate-600 font-medium">Column</label>
          <select
            value={draft.target_column ?? ''}
            onChange={e => {
              const col = COMPUTABLE_COLS.find(c => c.key === e.target.value);
              onChange({ ...draft, target_column: e.target.value, column_label: col?.label });
            }}
            className="mt-1 w-full text-sm border border-slate-300 rounded-lg px-3 py-1.5 focus:outline-none focus:ring-2 focus:ring-blue-400 bg-white"
          >
            {COMPUTABLE_COLS.map(c => (
              <option key={c.key} value={c.key}>{c.label} ({c.key})</option>
            ))}
          </select>
        </div>

        {/* Condition type */}
        <div>
          <label className="text-xs text-slate-600 font-medium">Applies To</label>
          <select
            value={draft.condition_type ?? 'all'}
            onChange={e => onChange({ ...draft, condition_type: e.target.value as DrFormulaRule['condition_type'] })}
            className="mt-1 w-full text-sm border border-slate-300 rounded-lg px-3 py-1.5 focus:outline-none focus:ring-2 focus:ring-blue-400 bg-white"
          >
            <option value="all">All items</option>
            <option value="only">Only these items:</option>
            <option value="except">All items EXCEPT:</option>
          </select>
        </div>

        {/* Condition items (when not "all") */}
        {draft.condition_type !== 'all' && (
          <div className="col-span-2">
            <label className="text-xs text-slate-600 font-medium">Item names (comma-separated)</label>
            <input type="text"
              value={draft.condition_items ?? ''}
              placeholder="GOLD,SILVER  or  GOLD(C)"
              onChange={e => onChange({ ...draft, condition_items: e.target.value })}
              className="mt-1 w-full text-sm border border-slate-300 rounded-lg px-3 py-1.5 focus:outline-none focus:ring-2 focus:ring-blue-400 bg-white"
            />
          </div>
        )}

        {/* Expression */}
        <div className="col-span-2">
          <label className="text-xs text-slate-600 font-medium">
            Formula Expression
            <span className="text-slate-400 font-normal ml-1">(leave empty for manual entry)</span>
          </label>
          <input type="text"
            value={draft.expression ?? ''}
            placeholder="e.g.  value * baggage_rate"
            onChange={e => { onChange({ ...draft, expression: e.target.value }); onPreview(e.target.value); }}
            className={`mt-1 w-full text-sm font-mono border rounded-lg px-3 py-1.5 focus:outline-none focus:ring-2 bg-white
              ${exprErr ? 'border-red-400 focus:ring-red-400' : 'border-slate-300 focus:ring-blue-400'}`}
          />
          {exprErr && <p className="text-xs text-red-600 mt-1">{exprErr}</p>}
          {preview && !exprErr && (
            <p className={`text-xs mt-1 flex items-center gap-1 ${preview.startsWith('Error') ? 'text-red-600' : 'text-emerald-700'}`}>
              {preview.startsWith('Error') ? <AlertTriangle size={11} /> : <CheckCircle size={11} />}
              {preview}
            </p>
          )}
        </div>

        {/* Active toggle */}
        <div className="flex items-center gap-2">
          <input type="checkbox" id="rule-active" checked={!!draft.is_active}
            onChange={e => onChange({ ...draft, is_active: e.target.checked })}
            className="w-4 h-4 accent-teal-600" />
          <label htmlFor="rule-active" className="text-xs text-slate-600">Active</label>
        </div>

        {/* Notes */}
        <div className="col-span-2">
          <label className="text-xs text-slate-600 font-medium">Notes (optional)</label>
          <input type="text" value={draft.notes ?? ''}
            placeholder="Explanation or reference"
            onChange={e => onChange({ ...draft, notes: e.target.value })}
            className="mt-1 w-full text-sm border border-slate-300 rounded-lg px-3 py-1.5 focus:outline-none focus:ring-2 focus:ring-blue-400 bg-white" />
        </div>
      </div>

      <div className="flex gap-3">
        <button onClick={onCancel}
          className="px-4 py-1.5 text-sm text-slate-600 bg-white border border-slate-300 rounded-lg hover:bg-slate-50">
          Cancel
        </button>
        <button onClick={onSave} disabled={!!exprErr}
          className="px-4 py-1.5 text-sm font-semibold bg-slate-800 hover:bg-slate-900 text-white rounded-lg disabled:opacity-50">
          Review &amp; Save
        </button>
      </div>
    </div>
  );
}
