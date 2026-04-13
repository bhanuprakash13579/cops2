/* eslint-disable @typescript-eslint/no-explicit-any */
import { useState, useEffect } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { ArrowLeft, Gavel, AlertCircle, CheckCircle, User, FileText } from 'lucide-react';
import DatePicker from '@/components/DatePicker';
import api from '@/lib/api';

const fmtDate = (d: string | null | undefined): string => {
  if (!d) return '—';
  const parts = d.split('-');
  if (parts.length === 3) return `${parts[2]}-${parts[1]}-${parts[0]}`;
  return d;
};

export default function OfflineAdjudicationComplete() {
  const { os_no, os_year } = useParams<{ os_no: string; os_year: string }>();
  const navigate = useNavigate();

  const [caseData, setCaseData] = useState<any>(null);
  const [loadError, setLoadError] = useState('');
  const [loadingCase, setLoadingCase] = useState(true);

  const [formData, setFormData] = useState({
    adj_offr_name: '',
    adj_designation: '',
    adjudication_date: new Date().toISOString().split('T')[0],
    rf_amount: '',
    pp_amount: '',
    ref_amount: '',
    conf_value: '',
    redeemed_value: '',
    re_export_value: '',
    adjn_offr_remarks: '',
    close_case: false,
  });

  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const [errorMsg, setErrorMsg] = useState('');
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [successSaved, setSuccessSaved] = useState(false);
  const [confirmSave, setConfirmSave] = useState(false);

  // ── Load case details ─────────────────────────────────────────────────────
  useEffect(() => {
    if (!os_no || !os_year) return;
    setLoadingCase(true);
    api.get(`/os/${os_no}/${os_year}`)
      .then(res => {
        setCaseData(res.data);
      })
      .catch(err => {
        setLoadError(err.response?.data?.detail || 'Failed to load case details.');
      })
      .finally(() => setLoadingCase(false));
  }, [os_no, os_year]);

  // Pre-populate form if already completed
  useEffect(() => {
    if (!caseData) return;
    if (caseData.adj_offr_name) {
      setFormData(prev => ({
        ...prev,
        adj_offr_name: caseData.adj_offr_name || '',
        adj_designation: caseData.adj_offr_designation || caseData.adj_designation || caseData.adjn_designation || '',
        adjudication_date: caseData.adjudication_date || prev.adjudication_date,
        rf_amount: caseData.rf_amount != null ? String(caseData.rf_amount) : '',
        pp_amount: caseData.pp_amount != null ? String(caseData.pp_amount) : '',
        ref_amount: caseData.ref_amount != null ? String(caseData.ref_amount) : '',
        conf_value: caseData.confiscated_value != null ? String(caseData.confiscated_value) : (caseData.conf_value != null ? String(caseData.conf_value) : ''),
        redeemed_value: caseData.redeemed_value != null ? String(caseData.redeemed_value) : '',
        re_export_value: caseData.re_export_value != null ? String(caseData.re_export_value) : '',
        adjn_offr_remarks: caseData.adjn_offr_remarks || '',
      }));
    }
  }, [caseData]);

  const isAlreadyCompleted = !!(caseData?.adj_offr_name);

  const clearFieldError = (field: string) => {
    setFieldErrors(prev => {
      if (!prev[field]) return prev;
      const { [field]: _removed, ...rest } = prev;
      return rest;
    });
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();

    if (!confirmSave) {
      setErrorMsg('Please tick the confirmation checkbox before saving.');
      return;
    }
    setErrorMsg('');
    setFieldErrors({});

    const errors: Record<string, string> = {};
    if (!formData.adj_offr_name.trim()) errors.adj_offr_name = 'Adjudicating Officer Name is required.';
    if (!formData.adj_designation.trim()) errors.adj_designation = 'Designation is required.';

    if (Object.keys(errors).length > 0) {
      setFieldErrors(errors);
      setErrorMsg('Please fill all mandatory fields.');
      return;
    }

    setIsSubmitting(true);
    try {
      const isDateValid = (d: string) => d && /^\d{4}-\d{2}-\d{2}$/.test(d);
      const payload: any = {
        adj_offr_name: formData.adj_offr_name.trim(),
        adj_offr_designation: formData.adj_designation.trim(),
      };
      if (formData.adjudication_date && isDateValid(formData.adjudication_date)) {
        payload.adjudication_date = formData.adjudication_date;
      }
      if (formData.rf_amount !== '') payload.rf_amount = Number(formData.rf_amount) || 0;
      if (formData.pp_amount !== '') payload.pp_amount = Number(formData.pp_amount) || 0;
      if (formData.ref_amount !== '') payload.ref_amount = Number(formData.ref_amount) || 0;
      if (formData.conf_value !== '') payload.confiscated_value = Number(formData.conf_value) || 0;
      if (formData.redeemed_value !== '') payload.redeemed_value = Number(formData.redeemed_value) || 0;
      if (formData.re_export_value !== '') payload.re_export_value = Number(formData.re_export_value) || 0;
      if (formData.adjn_offr_remarks.trim()) payload.adjn_offr_remarks = formData.adjn_offr_remarks.trim();
      if (formData.close_case) payload.close_case = true;

      await api.patch(`/os/${os_no}/${os_year}/complete-offline-adj`, payload);
      setSuccessSaved(true);
    } catch (err: any) {
      let errMsg = err.response?.data?.detail || err.message || 'Failed to save adjudication details.';
      if (Array.isArray(errMsg)) errMsg = errMsg.map((e: any) => `${e.loc?.join('.')} - ${e.msg}`).join(', ');
      else if (typeof errMsg === 'object') errMsg = JSON.stringify(errMsg);
      setErrorMsg(errMsg);
    } finally {
      setIsSubmitting(false);
    }
  };

  // ── Loading state ─────────────────────────────────────────────────────────
  if (loadingCase) {
    return (
      <div className="flex items-center justify-center min-h-64 pt-10">
        <div className="flex flex-col items-center gap-3 text-slate-500">
          <div className="w-8 h-8 border-4 border-amber-200 border-t-amber-600 rounded-full animate-spin"></div>
          <span className="font-medium text-sm">Loading case details...</span>
        </div>
      </div>
    );
  }

  if (loadError) {
    return (
      <div className="space-y-4 w-full">
        <div className="flex items-center bg-white px-4 py-3 border-b border-slate-200 rounded-xl border">
          <button onClick={() => navigate('/adjudication/offline-pending')} className="p-2 bg-slate-50 border border-slate-200 rounded-md hover:bg-slate-100 transition-colors mr-4">
            <ArrowLeft size={20} className="text-slate-600" />
          </button>
          <h1 className="text-xl font-bold text-slate-800">Complete Offline Adjudication</h1>
        </div>
        <div className="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded-lg flex items-center gap-3">
          <AlertCircle size={20} className="shrink-0" />
          <p className="text-sm">{loadError}</p>
        </div>
      </div>
    );
  }

  // ── Success state ─────────────────────────────────────────────────────────
  if (successSaved) {
    return (
      <div className="space-y-4 w-full pb-20">
        <div className="flex items-center bg-white px-4 py-3 border-b border-slate-200 rounded-xl border">
          <button onClick={() => navigate('/adjudication/offline-pending')} className="p-2 bg-slate-50 border border-slate-200 rounded-md hover:bg-slate-100 transition-colors mr-4">
            <ArrowLeft size={20} className="text-slate-600" />
          </button>
          <h1 className="text-xl font-bold text-slate-800">Complete Offline Adjudication</h1>
        </div>
        <div className="bg-white rounded-xl border border-green-200 p-10 text-center space-y-4 max-w-xl mx-auto mt-8">
          <CheckCircle size={48} className="text-green-500 mx-auto" />
          <h2 className="text-xl font-bold text-slate-800">Adjudication Details Saved</h2>
          <p className="text-slate-600 text-sm">
            Officer details for case{' '}
            <span className="font-bold text-amber-700">O.S. {os_no}/{os_year}</span>{' '}
            have been recorded successfully.
          </p>
          <div className="pt-2">
            <button
              onClick={() => navigate('/adjudication/offline-pending')}
              className="px-5 py-2 bg-amber-700 text-white font-semibold rounded-lg hover:bg-amber-600 transition-colors text-sm"
            >
              Back to List
            </button>
          </div>
        </div>
      </div>
    );
  }

  const totalItemsValue = caseData?.total_items_value || caseData?.items?.reduce((acc: number, itm: any) => acc + Number(itm.items_value || 0), 0) || 0;
  const itemCount = caseData?.total_items || caseData?.items?.length || 0;

  return (
    <div className="space-y-4 pt-2 w-full px-2 pb-20">
      {/* Header */}
      <div className="flex justify-between items-center bg-white px-4 py-3 border-b border-slate-200 rounded-xl border">
        <div className="flex items-center space-x-4">
          <button onClick={() => navigate('/adjudication/offline-pending')} className="p-2 bg-slate-50 border border-slate-200 rounded-md hover:bg-slate-100 transition-colors">
            <ArrowLeft size={20} className="text-slate-600" />
          </button>
          <div>
            <h1 className="text-2xl font-bold text-slate-800 tracking-tight">Complete Offline Adjudication</h1>
            <p className="text-slate-500 text-sm mt-0.5">O.S. {os_no}/{os_year} — Fill in the adjudicating officer details</p>
          </div>
        </div>
        {isAlreadyCompleted && (
          <span className="bg-green-100 text-green-800 border border-green-300 px-3 py-1.5 rounded-lg text-xs font-bold flex items-center gap-1.5">
            <CheckCircle size={13} /> ALREADY COMPLETED
          </span>
        )}
      </div>

      {isAlreadyCompleted && (
        <div className="bg-blue-50 border border-blue-200 text-blue-700 px-4 py-3 rounded-lg flex items-center gap-3">
          <AlertCircle size={18} className="shrink-0" />
          <p className="text-sm font-medium">This case has already been completed. You may view or update the details below.</p>
        </div>
      )}

      {errorMsg && (
        <div className="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded-lg flex items-start gap-3">
          <AlertCircle className="shrink-0 mt-0.5" size={20} />
          <div>
            <h4 className="font-bold text-sm">Error</h4>
            <p className="text-sm">{errorMsg}</p>
          </div>
        </div>
      )}

      {/* ── Case Summary (read-only) ───────────────────────────────────────── */}
      {caseData && (
        <div className="bg-white p-5 rounded-xl border border-slate-200 relative overflow-hidden">
          <div className="absolute top-0 left-0 w-1 h-full bg-amber-500"></div>
          <h2 className="text-sm font-bold text-slate-800 uppercase tracking-wider mb-4 border-b border-slate-100 pb-2 flex items-center">
            <FileText className="mr-2 text-amber-500" size={16} /> Case Summary
          </h2>
          <div className="grid grid-cols-2 md:grid-cols-3 xl:grid-cols-5 gap-4 text-sm">
            <div>
              <p className="text-xs font-semibold text-slate-500 uppercase mb-0.5">O.S. Ref</p>
              <p className="font-bold text-amber-800">{caseData.os_no}/{caseData.os_year}</p>
            </div>
            <div>
              <p className="text-xs font-semibold text-slate-500 uppercase mb-0.5">O.S. Date</p>
              <p className="font-medium text-slate-700">{fmtDate(caseData.os_date)}</p>
            </div>
            <div>
              <p className="text-xs font-semibold text-slate-500 uppercase mb-0.5">Passenger Name</p>
              <p className="font-bold text-slate-800">{caseData.pax_name || '—'}</p>
            </div>
            <div>
              <p className="text-xs font-semibold text-slate-500 uppercase mb-0.5">Passport No.</p>
              <p className="font-mono text-slate-700">{caseData.passport_no || '—'}</p>
            </div>
            <div>
              <p className="text-xs font-semibold text-slate-500 uppercase mb-0.5">Flight No.</p>
              <p className="font-medium text-slate-700">{caseData.flight_no || '—'}</p>
            </div>
            <div>
              <p className="text-xs font-semibold text-slate-500 uppercase mb-0.5">Adj. Type</p>
              <span className={`text-xs font-bold px-2 py-1 rounded-md ${caseData.file_spot === 'File' ? 'bg-purple-100 text-purple-800 border border-purple-200' : 'bg-blue-100 text-blue-800 border border-blue-200'}`}>
                {caseData.file_spot === 'File' ? 'ADJ. VIDE FILE' : 'SPOT'}
              </span>
            </div>
            <div className="col-span-2 md:col-span-2 xl:col-span-2">
              <p className="text-xs font-semibold text-slate-500 uppercase mb-0.5">Item Summary</p>
              <p className="font-medium text-slate-700">
                {itemCount} item(s) — Total Value:{' '}
                <span className="font-bold text-slate-800">
                  ₹ {totalItemsValue.toLocaleString('en-IN', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                </span>
              </p>
            </div>
          </div>

          {/* Items list */}
          {caseData.items && caseData.items.length > 0 && (
            <div className="mt-4 overflow-auto">
              <table className="w-full text-xs border-collapse">
                <thead>
                  <tr className="bg-slate-50 border-b border-slate-200">
                    <th className="px-3 py-1.5 text-left font-semibold text-slate-500 uppercase tracking-wide">S.No</th>
                    <th className="px-3 py-1.5 text-left font-semibold text-slate-500 uppercase tracking-wide">Description</th>
                    <th className="px-3 py-1.5 text-center font-semibold text-slate-500 uppercase tracking-wide">Qty</th>
                    <th className="px-3 py-1.5 text-right font-semibold text-slate-500 uppercase tracking-wide">Value (₹)</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-100">
                  {caseData.items.map((itm: any, i: number) => (
                    <tr key={i} className="hover:bg-slate-50">
                      <td className="px-3 py-1.5 text-slate-500">{i + 1}</td>
                      <td className="px-3 py-1.5 font-medium text-slate-800 uppercase">{itm.items_desc}</td>
                      <td className="px-3 py-1.5 text-center text-slate-600">{itm.items_qty} {itm.items_uqc}</td>
                      <td className="px-3 py-1.5 text-right text-slate-700 font-medium">
                        ₹ {Number(itm.items_value || 0).toLocaleString('en-IN', { maximumFractionDigits: 2 })}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      {/* ── Adjudication Officer Form ─────────────────────────────────────── */}
      <form
        onSubmit={handleSubmit}
        onKeyDown={e => { if (e.key === 'Enter' && (e.target as HTMLElement).tagName !== 'BUTTON') e.preventDefault(); }}
        className="space-y-6"
      >
        <div className="bg-white p-5 rounded-xl border border-slate-200 relative overflow-hidden">
          <div className="absolute top-0 left-0 w-1 h-full bg-amber-600"></div>
          <h2 className="text-sm font-bold text-slate-800 uppercase tracking-wider mb-4 border-b border-slate-100 pb-2 flex items-center">
            <User className="mr-2 text-amber-600" size={16} /> Adjudicating Officer Details
          </h2>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-5">
            {/* Mandatory fields */}
            <div>
              <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">
                Adjudicating Officer Name <span className="text-red-500">*</span>
              </label>
              <input
                id="field-adj_offr_name"
                type="text"
                className={`w-full px-3 py-2 border ${fieldErrors.adj_offr_name ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-amber-500'} rounded focus:ring-2 text-sm`}
                value={formData.adj_offr_name}
                onChange={e => { setFormData(prev => ({ ...prev, adj_offr_name: e.target.value })); clearFieldError('adj_offr_name'); }}
                readOnly={isAlreadyCompleted}
              />
              {fieldErrors.adj_offr_name && <p className="mt-1 text-xs font-semibold text-red-600">{fieldErrors.adj_offr_name}</p>}
            </div>
            <div>
              <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">
                Designation <span className="text-red-500">*</span>
              </label>
              <input
                id="field-adj_designation"
                type="text"
                className={`w-full px-3 py-2 border ${fieldErrors.adj_designation ? 'border-red-500 ring-1 ring-red-500' : 'border-slate-300 focus:ring-amber-500'} rounded focus:ring-2 text-sm`}
                value={formData.adj_designation}
                onChange={e => { setFormData(prev => ({ ...prev, adj_designation: e.target.value })); clearFieldError('adj_designation'); }}
                readOnly={isAlreadyCompleted}
              />
              {fieldErrors.adj_designation && <p className="mt-1 text-xs font-semibold text-red-600">{fieldErrors.adj_designation}</p>}
            </div>

            {/* Optional fields */}
            <div>
              <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">Adjudication Date</label>
              <DatePicker
                value={formData.adjudication_date}
                onChange={d => setFormData(prev => ({ ...prev, adjudication_date: d }))}
                inputClassName="w-full px-3 py-2 bg-slate-50 border border-slate-300 focus:ring-amber-500 rounded focus:ring-2 text-sm"
              />
            </div>

            <div>
              <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">Redemption Fine (₹)</label>
              <input
                type="text"
                inputMode="decimal"
                className="w-full px-3 py-2 bg-slate-50 border border-slate-300 rounded focus:ring-2 focus:ring-amber-500 text-sm text-right"
                value={formData.rf_amount}
                onChange={e => setFormData(prev => ({ ...prev, rf_amount: e.target.value.replace(/[^\d.]/g, '') }))}
                placeholder="0"
                readOnly={isAlreadyCompleted}
              />
            </div>

            <div>
              <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">Personal Penalty (₹)</label>
              <input
                type="text"
                inputMode="decimal"
                className="w-full px-3 py-2 bg-slate-50 border border-slate-300 rounded focus:ring-2 focus:ring-amber-500 text-sm text-right"
                value={formData.pp_amount}
                onChange={e => setFormData(prev => ({ ...prev, pp_amount: e.target.value.replace(/[^\d.]/g, '') }))}
                placeholder="0"
                readOnly={isAlreadyCompleted}
              />
            </div>

            <div>
              <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">Re-Export Fine (₹)</label>
              <input
                type="text"
                inputMode="decimal"
                className="w-full px-3 py-2 bg-slate-50 border border-slate-300 rounded focus:ring-2 focus:ring-amber-500 text-sm text-right"
                value={formData.ref_amount}
                onChange={e => setFormData(prev => ({ ...prev, ref_amount: e.target.value.replace(/[^\d.]/g, '') }))}
                placeholder="0"
                readOnly={isAlreadyCompleted}
              />
            </div>

            <div>
              <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">Confiscated Value (₹)</label>
              <input
                type="text"
                inputMode="decimal"
                className="w-full px-3 py-2 bg-slate-50 border border-slate-300 rounded focus:ring-2 focus:ring-amber-500 text-sm text-right"
                value={formData.conf_value}
                onChange={e => setFormData(prev => ({ ...prev, conf_value: e.target.value.replace(/[^\d.]/g, '') }))}
                placeholder="0"
                readOnly={isAlreadyCompleted}
              />
            </div>

            <div>
              <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">Redeemed Value (₹)</label>
              <input
                type="text"
                inputMode="decimal"
                className="w-full px-3 py-2 bg-slate-50 border border-slate-300 rounded focus:ring-2 focus:ring-amber-500 text-sm text-right"
                value={formData.redeemed_value}
                onChange={e => setFormData(prev => ({ ...prev, redeemed_value: e.target.value.replace(/[^\d.]/g, '') }))}
                placeholder="0"
                readOnly={isAlreadyCompleted}
              />
            </div>

            <div>
              <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">Re-Export Value (₹)</label>
              <input
                type="text"
                inputMode="decimal"
                className="w-full px-3 py-2 bg-slate-50 border border-slate-300 rounded focus:ring-2 focus:ring-amber-500 text-sm text-right"
                value={formData.re_export_value}
                onChange={e => setFormData(prev => ({ ...prev, re_export_value: e.target.value.replace(/[^\d.]/g, '') }))}
                placeholder="0"
                readOnly={isAlreadyCompleted}
              />
            </div>

            <div className="md:col-span-2">
              <label className="block text-xs font-semibold text-slate-500 uppercase mb-1">Adjudication Remarks</label>
              <textarea
                rows={3}
                className="w-full px-3 py-2 bg-slate-50 border border-slate-300 rounded focus:ring-2 focus:ring-amber-500 text-sm resize-none"
                value={formData.adjn_offr_remarks}
                onChange={e => setFormData(prev => ({ ...prev, adjn_offr_remarks: e.target.value }))}
                placeholder="Optional adjudication remarks..."
                readOnly={isAlreadyCompleted}
              />
            </div>

            {!isAlreadyCompleted && (
              <div className="md:col-span-2">
                <label className="flex items-center gap-2.5 cursor-pointer select-none">
                  <input
                    type="checkbox"
                    className="w-4 h-4 rounded border-slate-300 text-amber-600 focus:ring-amber-500"
                    checked={formData.close_case}
                    onChange={e => setFormData(prev => ({ ...prev, close_case: e.target.checked }))}
                  />
                  <span className="text-sm font-medium text-slate-700">Close Case</span>
                  <span className="text-xs text-slate-400">(Mark case as fully adjudicated and closed)</span>
                </label>
              </div>
            )}
          </div>
        </div>

        {/* ── Submit bar ────────────────────────────────────────────────────── */}
        {!isAlreadyCompleted && (
          <div className="border-t border-slate-200 pt-4 mt-2">
            <label className="flex items-center gap-2.5 cursor-pointer select-none mb-3">
              <input
                type="checkbox"
                className="w-4 h-4 rounded border-slate-300 text-amber-600 focus:ring-amber-500"
                checked={confirmSave}
                onChange={e => setConfirmSave(e.target.checked)}
              />
              <span className="text-sm font-medium text-slate-700">
                I confirm the above details are correct and wish to save.
              </span>
            </label>
          </div>
        )}
        <div className="flex items-center justify-between pt-2">
          <button
            type="button"
            onClick={() => navigate('/adjudication/offline-pending')}
            className="flex items-center gap-2 px-4 py-2 border border-slate-300 bg-white text-slate-700 font-semibold rounded-lg hover:bg-slate-50 transition-colors text-sm"
          >
            <ArrowLeft size={15} /> Back to List
          </button>
          {!isAlreadyCompleted && (
            <button
              type="submit"
              disabled={isSubmitting || !confirmSave}
              className="px-6 py-2 bg-amber-700 text-white font-semibold rounded-lg hover:bg-amber-600 transition-colors text-sm disabled:opacity-60 flex items-center gap-2"
            >
              {isSubmitting ? (
                <>
                  <span className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin"></span>
                  Saving...
                </>
              ) : (
                <>
                  <Gavel size={15} /> Save Adjudication Details
                </>
              )}
            </button>
          )}
        </div>
      </form>
    </div>
  );
}
