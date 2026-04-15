import { useState, useEffect } from 'react';
import { Scale, Pencil, Save, X, ShieldCheck, ShieldAlert, FileText } from 'lucide-react';
import api from '@/lib/api';

interface Statute {
  id: number;
  keyword: string;
  display_name: string;
  is_prohibited: boolean;
  supdt_goods_clause: string;
  adjn_goods_clause: string;
  legal_reference: string;
}

interface RemarksTemplate {
  id: number | null;
  label: string;
  value: string;
}

export default function StatutesAdmin({ adminToken }: { adminToken: string }) {
  const [statutes, setStatutes] = useState<Statute[]>([]);
  const [loading, setLoading] = useState(true);
  const [editingKey, setEditingKey] = useState<number | null>(null);
  const [editData, setEditData] = useState<Partial<Statute>>({});
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState('');

  // Remarks templates state
  const [templates, setTemplates] = useState<Record<string, RemarksTemplate>>({});
  const [editingTplKey, setEditingTplKey] = useState<string | null>(null);
  const [tplDraft, setTplDraft] = useState('');
  const [savingTpl, setSavingTpl] = useState(false);
  const [tplSaveError, setTplSaveError] = useState('');

  useEffect(() => {
    api.get('/statutes').then(res => {
      setStatutes(res.data);
      setLoading(false);
    }).catch(() => {
      setLoading(false);
    });
    api.get('/admin/config/remarks-templates').then(res => {
      setTemplates(res.data);
    }).catch(() => {});
  }, []);

  const startEdit = (s: Statute) => {
    setEditingKey(s.id);
    setEditData({
      display_name: s.display_name,
      is_prohibited: s.is_prohibited,
      supdt_goods_clause: s.supdt_goods_clause,
      adjn_goods_clause: s.adjn_goods_clause,
      legal_reference: s.legal_reference,
    });
  };

  const cancelEdit = () => {
    setEditingKey(null);
    setEditData({});
  };

  const saveEdit = async () => {
    if (!editingKey) return;
    setSaving(true);
    setSaveError('');
    try {
      await api.put(`/statutes/${editingKey}`, editData, {
        headers: { Authorization: `Bearer ${adminToken}` },
      });
      setStatutes(prev => prev.map(s => s.id === editingKey ? { ...s, ...editData } as Statute : s));
      setEditingKey(null);
      setEditData({});
    } catch (err: any) {
      setSaveError(err?.response?.data?.detail || 'Save failed. Please try again.');
    } finally {
      setSaving(false);
    }
  };

  const saveTpl = async (key: string) => {
    setSavingTpl(true);
    setTplSaveError('');
    try {
      await api.put(`/admin/config/remarks-templates/${key}`, { template_text: tplDraft }, {
        headers: { Authorization: `Bearer ${adminToken}` },
      });
      setTemplates(prev => ({ ...prev, [key]: { ...prev[key], value: tplDraft } }));
      setEditingTplKey(null);
    } catch (err: any) {
      setTplSaveError(err?.response?.data?.detail || 'Save failed. Please try again.');
    } finally {
      setSavingTpl(false);
    }
  };

  if (loading) {
    return <div className="text-center py-8 text-slate-500 text-sm">Loading statutes...</div>;
  }

  // Render one editable template row
  const TplRow = ({ tplKey, rows = 3 }: { tplKey: string; rows?: number }) => {
    const tpl = templates[tplKey];
    const label = tpl?.label ?? tplKey;
    const value = tpl?.value ?? '';
    const isEditing = editingTplKey === tplKey;
    return (
      <div className="space-y-1.5">
        <div className="flex items-center justify-between">
          <p className="text-[11px] font-bold text-slate-600 uppercase tracking-wider">{label}</p>
          {!isEditing ? (
            <button
              onClick={() => { setEditingTplKey(tplKey); setTplDraft(value); }}
              className="text-[11px] px-2 py-1 bg-white border border-slate-300 text-slate-600 rounded hover:bg-slate-50 flex items-center gap-1 font-semibold"
            >
              <Pencil size={11} /> Edit
            </button>
          ) : (
            <div className="flex items-center gap-1.5 flex-wrap">
              <button
                onClick={() => saveTpl(tplKey)}
                disabled={savingTpl}
                className="text-[11px] px-2 py-1 bg-indigo-600 text-white rounded hover:bg-indigo-700 flex items-center gap-1 font-semibold disabled:opacity-50"
              >
                <Save size={11} /> Save
              </button>
              <button
                onClick={() => setEditingTplKey(null)}
                className="text-[11px] px-2 py-1 bg-white border border-slate-300 text-slate-600 rounded hover:bg-slate-50 flex items-center gap-1 font-semibold"
              >
                <X size={11} /> Cancel
              </button>
              {tplSaveError && <span className="text-[11px] text-red-600 font-medium">{tplSaveError}</span>}
            </div>
          )}
        </div>
        {isEditing ? (
          <textarea
            rows={rows}
            className="w-full px-3 py-2 border border-indigo-300 rounded text-sm resize-none focus:outline-none focus:ring-2 focus:ring-indigo-400"
            value={tplDraft}
            onChange={e => setTplDraft(e.target.value)}
          />
        ) : (
          <p className="text-xs text-slate-700 bg-slate-50 rounded px-3 py-2 leading-relaxed">
            {value || <span className="italic text-slate-400">Using built-in default</span>}
          </p>
        )}
      </div>
    );
  };

  return (
    <div className="space-y-4">

      {/* ── Remarks Templates — Arrival Cases ── */}
      <div className="border border-indigo-200 rounded-xl overflow-hidden bg-white shadow-sm">
        <div className="flex items-center gap-2 px-4 py-3 bg-indigo-50 border-b border-indigo-200">
          <FileText size={16} className="text-indigo-600" />
          <div>
            <h3 className="text-sm font-bold text-indigo-800">Remarks Templates — Arrival Cases</h3>
            <p className="text-[10px] text-indigo-600 mt-0.5">
              SUPDT opening uses <code className="bg-indigo-100 px-1 rounded">{'{date}'}</code> <code className="bg-indigo-100 px-1 rounded">{'{city}'}</code> <code className="bg-indigo-100 px-1 rounded">{'{flight_no}'}</code>.
              AC disposal uses <code className="bg-indigo-100 px-1 rounded">{'{items}'}</code> for the item list.
            </p>
          </div>
        </div>
        <div className="p-4 space-y-5">

          {/* SUPDT Opening */}
          <div>
            <p className="text-[10px] font-bold text-slate-500 uppercase tracking-widest mb-2.5">SUPDT — Opening Paragraph</p>
            <TplRow tplKey="remarks_import_supdt_opening" rows={2} />
          </div>

          <div className="border-t border-slate-100 pt-4">
            <p className="text-[10px] font-bold text-slate-500 uppercase tracking-widest mb-2.5">SUPDT — Closing Paragraph (indirect language; section numbers signal the outcome)</p>
            <div className="space-y-4">
              {/* CONFS */}
              <div className="rounded-lg border border-red-100 bg-red-50/40 p-3 space-y-2">
                <span className="text-[10px] font-bold text-red-700 uppercase tracking-wider">Absolute Confiscation (CONFS) — prohibited goods · Sec 111(d)(l)(m)(o)</span>
                <TplRow tplKey="remarks_supdt_import_confs_closing" rows={4} />
              </div>
              {/* RF */}
              <div className="rounded-lg border border-amber-100 bg-amber-50/40 p-3 space-y-2">
                <span className="text-[10px] font-bold text-amber-700 uppercase tracking-wider">Redemption Fine (RF) — commercial/over-allowance goods · Sec 111(d)(l)(m)</span>
                <TplRow tplKey="remarks_supdt_import_rf_closing" rows={4} />
              </div>
              {/* REF */}
              <div className="rounded-lg border border-blue-100 bg-blue-50/40 p-3 space-y-2">
                <span className="text-[10px] font-bold text-blue-700 uppercase tracking-wider">Re-export (REF) — restricted goods · Sec 111(d)(l)(m)(o)</span>
                <TplRow tplKey="remarks_supdt_import_ref_closing" rows={4} />
              </div>
            </div>
          </div>

          <div className="border-t border-slate-100 pt-4">
            <p className="text-[10px] font-bold text-slate-500 uppercase tracking-widest mb-2.5">AC — Disposal Order (explicit orders; <code className="bg-slate-100 px-1 rounded normal-case">{'{items}'}</code> → actual item names)</p>
            <div className="space-y-4">
              {/* CONFS */}
              <div className="rounded-lg border border-red-100 bg-red-50/40 p-3 space-y-2">
                <span className="text-[10px] font-bold text-red-700 uppercase tracking-wider">Absolute Confiscation (CONFS)</span>
                <TplRow tplKey="remarks_ac_import_confs_disposal" rows={3} />
              </div>
              {/* RF */}
              <div className="rounded-lg border border-amber-100 bg-amber-50/40 p-3 space-y-2">
                <span className="text-[10px] font-bold text-amber-700 uppercase tracking-wider">Redemption Fine (RF)</span>
                <TplRow tplKey="remarks_ac_import_rf_disposal" rows={3} />
              </div>
              {/* REF */}
              <div className="rounded-lg border border-blue-100 bg-blue-50/40 p-3 space-y-2">
                <span className="text-[10px] font-bold text-blue-700 uppercase tracking-wider">Re-export (REF)</span>
                <TplRow tplKey="remarks_ac_import_ref_disposal" rows={3} />
              </div>
            </div>
          </div>

        </div>
      </div>

      {/* ── Remarks Templates — Departure Cases ── */}
      <div className="border border-violet-200 rounded-xl overflow-hidden bg-white shadow-sm">
        <div className="flex items-center gap-2 px-4 py-3 bg-violet-50 border-b border-violet-200">
          <FileText size={16} className="text-violet-600" />
          <div>
            <h3 className="text-sm font-bold text-violet-800">Remarks Templates — Departure Cases</h3>
            <p className="text-[10px] text-violet-600 mt-0.5">
              SUPDT opening uses <code className="bg-violet-100 px-1 rounded">{'{date}'}</code> <code className="bg-violet-100 px-1 rounded">{'{city}'}</code> <code className="bg-violet-100 px-1 rounded">{'{flight_no}'}</code>.
              AC disposal uses <code className="bg-violet-100 px-1 rounded">{'{items}'}</code> for the item list.
              Re-export (REF) does not apply for departure cases.
            </p>
          </div>
        </div>
        <div className="p-4 space-y-5">

          {/* SUPDT Opening */}
          <div>
            <p className="text-[10px] font-bold text-slate-500 uppercase tracking-widest mb-2.5">SUPDT — Opening Paragraph</p>
            <TplRow tplKey="remarks_export_supdt_opening" rows={2} />
          </div>

          <div className="border-t border-slate-100 pt-4">
            <p className="text-[10px] font-bold text-slate-500 uppercase tracking-widest mb-2.5">SUPDT — Closing Paragraph</p>
            <div className="space-y-4">
              {/* CONFS */}
              <div className="rounded-lg border border-red-100 bg-red-50/40 p-3 space-y-2">
                <span className="text-[10px] font-bold text-red-700 uppercase tracking-wider">Absolute Confiscation (CONFS) — prohibited export · Sec 113</span>
                <TplRow tplKey="remarks_supdt_export_confs_closing" rows={4} />
              </div>
              {/* RF */}
              <div className="rounded-lg border border-amber-100 bg-amber-50/40 p-3 space-y-2">
                <span className="text-[10px] font-bold text-amber-700 uppercase tracking-wider">Redemption Fine (RF) — excess currency / restricted export · Sec 113</span>
                <TplRow tplKey="remarks_supdt_export_rf_closing" rows={4} />
              </div>
            </div>
          </div>

          <div className="border-t border-slate-100 pt-4">
            <p className="text-[10px] font-bold text-slate-500 uppercase tracking-widest mb-2.5">AC — Disposal Order (<code className="bg-slate-100 px-1 rounded normal-case">{'{items}'}</code> → actual item names)</p>
            <div className="space-y-4">
              {/* CONFS */}
              <div className="rounded-lg border border-red-100 bg-red-50/40 p-3 space-y-2">
                <span className="text-[10px] font-bold text-red-700 uppercase tracking-wider">Absolute Confiscation (CONFS)</span>
                <TplRow tplKey="remarks_ac_export_confs_disposal" rows={3} />
              </div>
              {/* RF */}
              <div className="rounded-lg border border-amber-100 bg-amber-50/40 p-3 space-y-2">
                <span className="text-[10px] font-bold text-amber-700 uppercase tracking-wider">Redemption Fine (RF)</span>
                <TplRow tplKey="remarks_ac_export_rf_disposal" rows={3} />
              </div>
            </div>
          </div>

        </div>
      </div>

      <div className="flex items-center justify-between mb-4">
        <div>
          <h3 className="text-lg font-bold text-slate-800 flex items-center">
            <Scale className="mr-2 text-indigo-600" size={20} /> Legal Statutes & Compliance
          </h3>
          <p className="text-xs text-slate-500 mt-0.5">
            Manage the legal clauses used by the Smart Remarks Auto-Generator. Changes apply immediately to new OS cases.
          </p>
        </div>
      </div>

      {statutes.map(s => (
        <div key={s.id} className="border border-slate-200 rounded-xl overflow-hidden bg-white shadow-sm">
          {/* Header */}
          <div className="flex items-center justify-between px-4 py-3 bg-slate-50 border-b border-slate-200">
            <div className="flex items-center gap-2">
              <span className="text-sm font-bold text-slate-800">{s.display_name}</span>
              <span className="text-[10px] font-mono px-1.5 py-0.5 bg-slate-200 text-slate-600 rounded">{s.keyword}</span>
              {s.is_prohibited ? (
                <span className="text-[10px] px-1.5 py-0.5 bg-red-100 text-red-700 border border-red-200 rounded font-bold flex items-center gap-0.5">
                  <ShieldAlert size={10} /> PROHIBITED
                </span>
              ) : (
                <span className="text-[10px] px-1.5 py-0.5 bg-green-100 text-green-700 border border-green-200 rounded font-bold flex items-center gap-0.5">
                  <ShieldCheck size={10} /> DUTIABLE
                </span>
              )}
            </div>
            {editingKey !== s.id ? (
              <button
                onClick={() => startEdit(s)}
                className="text-[11px] px-2 py-1 bg-white border border-slate-300 text-slate-600 rounded hover:bg-slate-50 flex items-center gap-1 font-semibold"
              >
                <Pencil size={12} /> Edit
              </button>
            ) : (
              <div className="flex items-center gap-1.5 flex-wrap">
                <button
                  onClick={saveEdit}
                  disabled={saving}
                  className="text-[11px] px-2 py-1 bg-brand-600 text-white rounded hover:bg-brand-700 flex items-center gap-1 font-semibold disabled:opacity-50"
                >
                  <Save size={12} /> Save
                </button>
                <button
                  onClick={cancelEdit}
                  className="text-[11px] px-2 py-1 bg-white border border-slate-300 text-slate-600 rounded hover:bg-slate-50 flex items-center gap-1 font-semibold"
                >
                  <X size={12} /> Cancel
                </button>
                {saveError && <span className="text-[11px] text-red-600 font-medium">{saveError}</span>}
              </div>
            )}
          </div>

          {/* Body */}
          {editingKey === s.id ? (
            <div className="p-4 space-y-3">
              <div>
                <label className="block text-[10px] font-bold text-slate-500 uppercase tracking-wider mb-1">Display Name</label>
                <input
                  type="text"
                  className="w-full px-3 py-1.5 border border-slate-300 rounded text-sm"
                  value={editData.display_name || ''}
                  onChange={e => setEditData(p => ({ ...p, display_name: e.target.value }))}
                />
              </div>
              <div className="flex items-center gap-2">
                <label className="text-[10px] font-bold text-slate-500 uppercase tracking-wider">Prohibited Item?</label>
                <input
                  type="checkbox"
                  className="w-4 h-4 accent-red-600 rounded"
                  checked={editData.is_prohibited || false}
                  onChange={e => setEditData(p => ({ ...p, is_prohibited: e.target.checked }))}
                />
                <span className="text-xs text-slate-500">{editData.is_prohibited ? 'Yes — Absolute Confiscation' : 'No — Redemption Fine / Duty'}</span>
              </div>
              <div>
                <label className="block text-[10px] font-bold text-slate-500 uppercase tracking-wider mb-1">SUPDT Goods Clause</label>
                <textarea
                  rows={3}
                  className="w-full px-3 py-1.5 border border-slate-300 rounded text-sm resize-none"
                  placeholder="One sentence describing why this item is seizable (for Superintendent's Remarks)..."
                  value={editData.supdt_goods_clause || ''}
                  onChange={e => setEditData(p => ({ ...p, supdt_goods_clause: e.target.value }))}
                />
              </div>
              <div>
                <label className="block text-[10px] font-bold text-slate-500 uppercase tracking-wider mb-1">AC Adjudication Clause</label>
                <textarea
                  rows={3}
                  className="w-full px-3 py-1.5 border border-slate-300 rounded text-sm resize-none"
                  placeholder="One sentence describing the legal finding for this item (for Adjudicating Officer's Order)..."
                  value={editData.adjn_goods_clause || ''}
                  onChange={e => setEditData(p => ({ ...p, adjn_goods_clause: e.target.value }))}
                />
              </div>
              <div>
                <label className="block text-[10px] font-bold text-slate-500 uppercase tracking-wider mb-1">Legal Reference</label>
                <input
                  type="text"
                  className="w-full px-3 py-1.5 border border-slate-300 rounded text-sm"
                  placeholder="Acts, notifications, circulars cited..."
                  value={editData.legal_reference || ''}
                  onChange={e => setEditData(p => ({ ...p, legal_reference: e.target.value }))}
                />
              </div>
            </div>
          ) : (
            <div className="p-4 space-y-2">
              <div className="grid grid-cols-2 gap-4">
                <div>
                  <p className="text-[10px] font-bold text-indigo-500 uppercase tracking-wider mb-0.5">SUPDT Clause</p>
                  <p className="text-xs text-slate-700 leading-relaxed">{s.supdt_goods_clause || '—'}</p>
                </div>
                <div>
                  <p className="text-[10px] font-bold text-amber-600 uppercase tracking-wider mb-0.5">AC Clause</p>
                  <p className="text-xs text-slate-700 leading-relaxed">{s.adjn_goods_clause || '—'}</p>
                </div>
              </div>
              <div>
                <p className="text-[10px] font-bold text-slate-400 uppercase tracking-wider mb-0.5">Legal Reference</p>
                <p className="text-xs text-slate-600">{s.legal_reference || '—'}</p>
              </div>
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
