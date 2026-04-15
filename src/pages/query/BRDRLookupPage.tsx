import { useState, useRef } from 'react';
import { Search, ArrowLeft, Receipt, FileX, Link2, ExternalLink, ChevronLeft, ChevronRight, Loader2 } from 'lucide-react';
import api from '@/lib/api';

// ── Types ──────────────────────────────────────────────────────────────────

type BrRow = {
  br_no: number; br_year: number | null; br_date: string | null;
  br_type: string; pax_name: string | null; passport_no: string | null;
  total_duty_paid: number; dr_no: string | null; os_no: string | null;
};
type DrRow = {
  dr_no: number; dr_year: number | null; dr_date: string | null;
  dr_type: string; pax_name: string | null; passport_no: string | null;
  total_items_value: number; closure_ind: string | null; os_no: string | null;
};
type LinkedDR = { dr_no: number; dr_year: number | null; dr_date: string | null; dr_type: string; pax_name: string | null; total_items_value: number; closure_ind: string | null; };
type LinkedBR = { br_no: number; br_year: number | null; br_date: string | null; br_type: string; total_duty_paid: number; };
type LinkedOS = { os_no: string; os_year: number; os_date: string | null; pax_name: string | null; status: string; total_items_value: number; };
type BrItem = { items_sno: number; items_desc: string | null; items_qty: number; items_uqc: string | null; items_value: number; items_fa: number; items_duty: number; items_duty_type: string | null; items_category: string | null; };
type DrItem = { items_sno: number; items_desc: string | null; items_qty: number; items_uqc: string | null; items_value: number; };
type BrDetail = BrRow & {
  br_shift: string | null; flight_no: string | null; flight_date: string | null;
  pax_nationality: string | null; passport_date: string | null;
  pax_address1: string | null; pax_address2: string | null; pax_address3: string | null;
  total_items_value: number; total_duty_amount: number; rf_amount: number; pp_amount: number; br_amount: number;
  challan_no: string | null; dr_date: string | null; os_date: string | null;
  batch_date: string | null; batch_shift: string | null; login_id: string | null;
  items: BrItem[]; linked_dr: LinkedDR | null; linked_os: LinkedOS | null;
};
type DrDetail = DrRow & {
  flight_no: string | null; flight_date: string | null;
  passport_date: string | null; pax_address1: string | null; pax_address2: string | null; pax_address3: string | null;
  closure_remarks: string | null; closure_date: string | null;
  unique_no: number | null; login_id: string | null;
  items: DrItem[]; linked_brs: LinkedBR[]; linked_os: LinkedOS | null;
};

// ── Helpers ────────────────────────────────────────────────────────────────

function fmtDate(d: string | null | undefined) {
  if (!d) return '—';
  try {
    const parts = d.split('-');
    if (parts.length !== 3) return d;
    const [y, m, dy] = parts;
    if (!y || !m || !dy) return d;
    return `${dy}/${m}/${y}`;
  } catch { return d; }
}
function fmtRs(n: number | undefined) {
  if (!n) return '—';
  return `₹${n.toLocaleString('en-IN', { minimumFractionDigits: 0, maximumFractionDigits: 2 })}`;
}

const BR_TYPES = ['Bagg', 'Gold', 'Silv', 'SDO', 'OOS', 'Fuel', 'TR'];
const DR_TYPES = ['Bagg', 'AIU', 'MHB', 'Other'];

function StatusBadge({ label, color }: { label: string; color: string }) {
  const map: Record<string, string> = {
    green: 'bg-emerald-100 text-emerald-700 border-emerald-200',
    red:   'bg-red-100 text-red-700 border-red-200',
    amber: 'bg-amber-100 text-amber-700 border-amber-200',
    blue:  'bg-blue-100 text-blue-700 border-blue-200',
    slate: 'bg-slate-100 text-slate-600 border-slate-200',
  };
  return <span className={`inline-flex items-center px-2 py-0.5 rounded text-[11px] font-semibold border ${map[color] ?? map.slate}`}>{label}</span>;
}

function ClosureBadge({ ind }: { ind: string | null }) {
  if (ind === 'Y') return <StatusBadge label="Closed" color="green" />;
  return <StatusBadge label="Open / Pending" color="amber" />;
}

function OSStatusBadge({ status }: { status: string }) {
  const c = status === 'Adjudicated' ? 'green' : status === 'Quashed' ? 'red' : status === 'Rejected' ? 'red' : 'amber';
  return <StatusBadge label={status} color={c} />;
}

// ── Detail Sections ────────────────────────────────────────────────────────

function InfoRow({ label, value }: { label: string; value: React.ReactNode }) {
  if (!value || value === '—') return null;
  return (
    <div className="flex gap-2 py-1 border-b border-slate-100 last:border-0">
      <span className="text-xs text-slate-500 w-36 shrink-0">{label}</span>
      <span className="text-xs text-slate-800 font-medium">{value}</span>
    </div>
  );
}

function LinkedCard({ title, color, children }: { title: string; color: string; children: React.ReactNode }) {
  const map: Record<string, string> = {
    blue:   'border-blue-200 bg-blue-50',
    amber:  'border-amber-200 bg-amber-50',
    purple: 'border-purple-200 bg-purple-50',
  };
  const title_map: Record<string, string> = {
    blue: 'text-blue-700', amber: 'text-amber-700', purple: 'text-purple-700',
  };
  return (
    <div className={`rounded-xl border p-4 space-y-2 ${map[color]}`}>
      <div className={`flex items-center gap-2 text-xs font-semibold uppercase tracking-wider ${title_map[color]}`}>
        <Link2 size={13} />
        {title}
      </div>
      {children}
    </div>
  );
}

function ItemsTable({ items, hasDuty }: { items: (BrItem | DrItem)[]; hasDuty: boolean }) {
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-xs text-left">
        <thead className="bg-slate-100 text-slate-500 uppercase tracking-wider">
          <tr>
            <th className="px-3 py-2">#</th>
            <th className="px-3 py-2">Description</th>
            <th className="px-3 py-2 text-right">Qty</th>
            <th className="px-3 py-2 text-right">Value (₹)</th>
            {hasDuty && <th className="px-3 py-2 text-right">Free Allow.</th>}
            {hasDuty && <th className="px-3 py-2 text-right">Duty (₹)</th>}
            {hasDuty && <th className="px-3 py-2">Category</th>}
          </tr>
        </thead>
        <tbody className="divide-y divide-slate-100">
          {items.map((it) => (
            <tr key={it.items_sno} className="hover:bg-slate-50">
              <td className="px-3 py-2 text-slate-500">{it.items_sno}</td>
              <td className="px-3 py-2 font-medium text-slate-700 max-w-xs">{it.items_desc || '—'}</td>
              <td className="px-3 py-2 text-right">{it.items_qty || '—'} {it.items_uqc || ''}</td>
              <td className="px-3 py-2 text-right">{it.items_value ? `₹${it.items_value.toLocaleString('en-IN')}` : '—'}</td>
              {hasDuty && <td className="px-3 py-2 text-right">{(it as BrItem).items_fa ? `₹${(it as BrItem).items_fa.toLocaleString('en-IN')}` : '—'}</td>}
              {hasDuty && <td className="px-3 py-2 text-right font-semibold text-emerald-700">{(it as BrItem).items_duty ? `₹${(it as BrItem).items_duty.toLocaleString('en-IN')}` : '—'}</td>}
              {hasDuty && <td className="px-3 py-2 text-slate-500">{(it as BrItem).items_category || '—'}</td>}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// ── BR Detail View ─────────────────────────────────────────────────────────

function BRDetailView({ br, onBack }: { br: BrDetail; onBack: () => void }) {
  const addr = [br.pax_address1, br.pax_address2, br.pax_address3].filter(Boolean).join(', ');
  return (
    <div className="space-y-4">
      <div className="flex items-center gap-3">
        <button onClick={onBack} className="p-2 rounded-lg hover:bg-slate-100 text-slate-500 transition-colors">
          <ArrowLeft size={18} />
        </button>
        <div>
          <h2 className="text-base font-bold text-slate-800">B.R. No. {br.br_no}/{br.br_year ?? '—'}</h2>
          <p className="text-xs text-slate-500">{br.br_type} · {fmtDate(br.br_date)} · {br.br_shift || '—'}</p>
        </div>
        <div className="ml-auto flex gap-2">
          {br.dr_no && <StatusBadge label={`DR: ${br.dr_no}`} color="amber" />}
          {br.os_no && <StatusBadge label={`OS: ${br.os_no}`} color="purple" />}
        </div>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {/* Passenger */}
        <div className="bg-white border border-slate-200 rounded-xl p-4 space-y-0.5">
          <p className="text-xs font-semibold text-slate-400 uppercase tracking-wider mb-2">Passenger</p>
          <InfoRow label="Name" value={br.pax_name} />
          <InfoRow label="Passport No." value={br.passport_no} />
          <InfoRow label="Passport Date" value={fmtDate(br.passport_date)} />
          <InfoRow label="Nationality" value={br.pax_nationality} />
          <InfoRow label="Address" value={addr || null} />
        </div>
        {/* Travel */}
        <div className="bg-white border border-slate-200 rounded-xl p-4 space-y-0.5">
          <p className="text-xs font-semibold text-slate-400 uppercase tracking-wider mb-2">Travel</p>
          <InfoRow label="Flight No." value={br.flight_no} />
          <InfoRow label="Flight Date" value={fmtDate(br.flight_date)} />
          <InfoRow label="Batch Date" value={fmtDate(br.batch_date)} />
          <InfoRow label="Batch Shift" value={br.batch_shift} />
          <InfoRow label="Entered By" value={br.login_id} />
        </div>
        {/* Financial */}
        <div className="bg-white border border-slate-200 rounded-xl p-4 space-y-0.5">
          <p className="text-xs font-semibold text-slate-400 uppercase tracking-wider mb-2">Payment</p>
          <InfoRow label="Goods Value" value={fmtRs(br.total_items_value)} />
          <InfoRow label="Total Duty" value={fmtRs(br.total_duty_amount)} />
          <InfoRow label="Redemption Fine" value={br.rf_amount ? fmtRs(br.rf_amount) : null} />
          <InfoRow label="Personal Penalty" value={br.pp_amount ? fmtRs(br.pp_amount) : null} />
          <InfoRow label="Total B.R. Amount" value={<span className="text-emerald-700 font-bold">{fmtRs(br.br_amount)}</span>} />
          <InfoRow label="Challan No." value={br.challan_no} />
        </div>
        {/* Cross references */}
        <div className="space-y-3">
          {br.linked_dr && (
            <LinkedCard title="Linked Detention Receipt" color="amber">
              <InfoRow label="D.R. No." value={`${br.linked_dr.dr_no}/${br.linked_dr.dr_year ?? '—'}`} />
              <InfoRow label="D.R. Date" value={fmtDate(br.linked_dr.dr_date)} />
              <InfoRow label="Type" value={br.linked_dr.dr_type} />
              <InfoRow label="Goods Value" value={fmtRs(br.linked_dr.total_items_value)} />
              <InfoRow label="Status" value={<ClosureBadge ind={br.linked_dr.closure_ind} />} />
            </LinkedCard>
          )}
          {br.linked_os && (
            <LinkedCard title="Linked O.S. Case" color="purple">
              <InfoRow label="O.S. No." value={`${br.linked_os.os_no}/${br.linked_os.os_year}`} />
              <InfoRow label="O.S. Date" value={fmtDate(br.linked_os.os_date)} />
              <InfoRow label="Goods Value" value={fmtRs(br.linked_os.total_items_value)} />
              <InfoRow label="Status" value={<OSStatusBadge status={br.linked_os.status} />} />
            </LinkedCard>
          )}
          {!br.linked_dr && !br.linked_os && (
            <div className="bg-slate-50 border border-slate-200 rounded-xl p-4 text-xs text-slate-400 text-center">
              No linked DR or OS case
            </div>
          )}
        </div>
      </div>

      {/* Items */}
      {br.items.length > 0 && (
        <div className="bg-white border border-slate-200 rounded-xl overflow-hidden">
          <div className="px-4 py-3 border-b border-slate-100">
            <p className="text-xs font-semibold text-slate-500 uppercase tracking-wider">Seized / Dutiable Items</p>
          </div>
          <ItemsTable items={br.items} hasDuty={true} />
        </div>
      )}
    </div>
  );
}

// ── DR Detail View ─────────────────────────────────────────────────────────

function DRDetailView({ dr, onBack }: { dr: DrDetail; onBack: () => void }) {
  const addr = [dr.pax_address1, dr.pax_address2, dr.pax_address3].filter(Boolean).join(', ');
  return (
    <div className="space-y-4">
      <div className="flex items-center gap-3">
        <button onClick={onBack} className="p-2 rounded-lg hover:bg-slate-100 text-slate-500 transition-colors">
          <ArrowLeft size={18} />
        </button>
        <div>
          <h2 className="text-base font-bold text-slate-800">D.R. No. {dr.dr_no}/{dr.dr_year ?? '—'}</h2>
          <p className="text-xs text-slate-500">{dr.dr_type} · {fmtDate(dr.dr_date)}</p>
        </div>
        <div className="ml-auto">
          <ClosureBadge ind={dr.closure_ind} />
        </div>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {/* Passenger */}
        <div className="bg-white border border-slate-200 rounded-xl p-4 space-y-0.5">
          <p className="text-xs font-semibold text-slate-400 uppercase tracking-wider mb-2">Passenger</p>
          <InfoRow label="Name" value={dr.pax_name} />
          <InfoRow label="Passport No." value={dr.passport_no} />
          <InfoRow label="Passport Date" value={fmtDate(dr.passport_date)} />
          <InfoRow label="Address" value={addr || null} />
        </div>
        {/* Travel */}
        <div className="bg-white border border-slate-200 rounded-xl p-4 space-y-0.5">
          <p className="text-xs font-semibold text-slate-400 uppercase tracking-wider mb-2">Travel</p>
          <InfoRow label="Flight No." value={dr.flight_no} />
          <InfoRow label="Flight Date" value={fmtDate(dr.flight_date)} />
          <InfoRow label="Goods Value" value={fmtRs(dr.total_items_value)} />
          <InfoRow label="Entered By" value={dr.login_id} />
        </div>
        {/* Closure */}
        <div className="bg-white border border-slate-200 rounded-xl p-4 space-y-0.5">
          <p className="text-xs font-semibold text-slate-400 uppercase tracking-wider mb-2">Closure</p>
          <InfoRow label="Status" value={<ClosureBadge ind={dr.closure_ind} />} />
          <InfoRow label="Closure Date" value={fmtDate(dr.closure_date)} />
          <InfoRow label="Remarks" value={dr.closure_remarks} />
        </div>
        {/* Cross references */}
        <div className="space-y-3">
          {dr.linked_brs.length > 0 && (
            <LinkedCard title={`Linked Baggage Receipt${dr.linked_brs.length > 1 ? 's' : ''}`} color="blue">
              {dr.linked_brs.map((b, i) => (
                <div key={i} className="text-xs space-y-0.5 pb-2 border-b border-blue-100 last:border-0 last:pb-0">
                  <InfoRow label="B.R. No." value={`${b.br_no}/${b.br_year ?? '—'}`} />
                  <InfoRow label="B.R. Date" value={fmtDate(b.br_date)} />
                  <InfoRow label="Type" value={b.br_type} />
                  <InfoRow label="Duty Paid" value={<span className="text-emerald-700 font-bold">{fmtRs(b.total_duty_paid)}</span>} />
                </div>
              ))}
            </LinkedCard>
          )}
          {dr.linked_os && (
            <LinkedCard title="Linked O.S. Case" color="purple">
              <InfoRow label="O.S. No." value={`${dr.linked_os.os_no}/${dr.linked_os.os_year}`} />
              <InfoRow label="O.S. Date" value={fmtDate(dr.linked_os.os_date)} />
              <InfoRow label="Goods Value" value={fmtRs(dr.linked_os.total_items_value)} />
              <InfoRow label="Status" value={<OSStatusBadge status={dr.linked_os.status} />} />
            </LinkedCard>
          )}
          {dr.linked_brs.length === 0 && !dr.linked_os && (
            <div className="bg-slate-50 border border-slate-200 rounded-xl p-4 text-xs text-slate-400 text-center">
              No linked B.R. or O.S. case
            </div>
          )}
        </div>
      </div>

      {/* Items */}
      {dr.items.length > 0 && (
        <div className="bg-white border border-slate-200 rounded-xl overflow-hidden">
          <div className="px-4 py-3 border-b border-slate-100">
            <p className="text-xs font-semibold text-slate-500 uppercase tracking-wider">Detained Items</p>
          </div>
          <ItemsTable items={dr.items} hasDuty={false} />
        </div>
      )}
    </div>
  );
}

// ── Main Page ─────────────────────────────────────────────────────────────

export default function BRDRLookupPage() {
  const [tab, setTab] = useState<'br' | 'dr'>('br');
  const [q, setQ] = useState('');
  const [year, setYear] = useState('');
  const [typeFilter, setTypeFilter] = useState('');
  const [page, setPage] = useState(1);

  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [brResults, setBrResults] = useState<BrRow[]>([]);
  const [drResults, setDrResults] = useState<DrRow[]>([]);
  const [total, setTotal] = useState(0);
  const [searched, setSearched] = useState(false);

  const [detailLoading, setDetailLoading] = useState(false);
  const [brDetail, setBrDetail] = useState<BrDetail | null>(null);
  const [drDetail, setDrDetail] = useState<DrDetail | null>(null);

  const topRef = useRef<HTMLDivElement>(null);
  const LIMIT = 50;

  const handleSearch = async (p = 1) => {
    setError(null);
    setLoading(true);
    setBrDetail(null);
    setDrDetail(null);
    setSearched(true);
    try {
      const params: Record<string, string | number> = { page: p, per_page: LIMIT };
      if (q.trim()) params.search = q.trim();
      const parsedYear = parseInt(year);
      if (year && !isNaN(parsedYear)) params.year = parsedYear;
      if (typeFilter) params[tab === 'br' ? 'br_type' : 'dr_type'] = typeFilter;

      const res = await api.get(`/os-query/${tab}/search`, { params });
      if (tab === 'br') setBrResults(res.data.items);
      else setDrResults(res.data.items);
      setTotal(res.data.total);
      setPage(p);
    } catch (err: any) {
      setError(err?.response?.data?.detail || 'Search failed. Please try again.');
    } finally {
      setLoading(false);
    }
  };

  const handleTabChange = (t: 'br' | 'dr') => {
    setTab(t);
    setQ(''); setYear(''); setTypeFilter('');
    setBrResults([]); setDrResults([]);
    setBrDetail(null); setDrDetail(null);
    setSearched(false); setTotal(0); setPage(1);
    setError(null);
  };

  const loadBrDetail = async (row: BrRow) => {
    setBrDetail(null);
    setError(null);
    setDetailLoading(true);
    try {
      const res = await api.get(`/os-query/br/${row.br_no}/${row.br_year ?? 0}`);
      setBrDetail(res.data);
    } catch (err: any) {
      if (err?.response?.status === 404)
        setError(`B.R. No. ${row.br_no} not found in the database.`);
      else
        setError(err?.response?.data?.detail || 'Failed to load B.R. details. Please try again.');
    } finally {
      setDetailLoading(false);
    }
  };

  const loadDrDetail = async (row: DrRow) => {
    setDrDetail(null);
    setError(null);
    setDetailLoading(true);
    try {
      const res = await api.get(`/os-query/dr/${row.dr_no}/${row.dr_year ?? 0}`);
      setDrDetail(res.data);
    } catch (err: any) {
      if (err?.response?.status === 404)
        setError(`D.R. No. ${row.dr_no} not found in the database.`);
      else
        setError(err?.response?.data?.detail || 'Failed to load D.R. details. Please try again.');
    } finally {
      setDetailLoading(false);
    }
  };

  const totalPages = Math.ceil(total / LIMIT);
  const results = tab === 'br' ? brResults : drResults;
  const showDetail = brDetail || drDetail;

  return (
    <div className="space-y-4 w-full pb-20" ref={topRef}>
      {/* Header */}
      <div className="bg-white border border-slate-200 rounded-xl px-5 py-4">
        <div className="flex items-center gap-3">
          <div className="p-2 bg-emerald-50 border border-emerald-100 rounded-lg">
            <Receipt size={20} className="text-emerald-600" />
          </div>
          <div>
            <h1 className="text-xl font-bold text-slate-800">B.R. / D.R. Lookup</h1>
            <p className="text-xs text-slate-500">View Baggage Receipts and Detention Receipts with their linked cases</p>
          </div>
        </div>
      </div>

      {/* Tabs */}
      <div className="flex gap-1 bg-slate-100 rounded-xl p-1 w-fit">
        {(['br', 'dr'] as const).map(t => (
          <button key={t} onClick={() => handleTabChange(t)}
            className={`px-5 py-2 rounded-lg text-sm font-semibold transition-colors ${tab === t ? 'bg-white text-slate-800 shadow-sm' : 'text-slate-500 hover:text-slate-700'}`}>
            {t === 'br' ? 'Baggage Receipt (B.R.)' : 'Detention Receipt (D.R.)'}
          </button>
        ))}
      </div>

      {/* Search bar */}
      <div className="bg-white border border-slate-200 rounded-xl p-4">
        <div className="flex flex-wrap gap-3 items-end">
          <div className="flex-1 min-w-48">
            <label className="block text-xs font-medium text-slate-500 mb-1">
              {tab === 'br' ? 'B.R. No., Passenger Name, or Passport No.' : 'D.R. No., Passenger Name, or Passport No.'}
            </label>
            <div className="relative">
              <Search size={15} className="absolute left-3 top-2.5 text-slate-400" />
              <input
                value={q} onChange={e => setQ(e.target.value)}
                onKeyDown={e => e.key === 'Enter' && handleSearch(1)}
                className="w-full pl-8 pr-3 py-2 border border-slate-300 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-emerald-500 focus:border-emerald-500"
                placeholder={tab === 'br' ? 'e.g. 11682 or JOHN or A1234567' : 'e.g. 929 or JOHN or A1234567'}
              />
            </div>
          </div>
          <div className="w-28">
            <label className="block text-xs font-medium text-slate-500 mb-1">Year</label>
            <input value={year} onChange={e => setYear(e.target.value.replace(/\D/g, '').slice(0, 4))}
              inputMode="numeric" pattern="[0-9]*"
              className="w-full px-3 py-2 border border-slate-300 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-emerald-500"
              placeholder="e.g. 2023" />
          </div>
          <div className="w-36">
            <label className="block text-xs font-medium text-slate-500 mb-1">Type</label>
            <select value={typeFilter} onChange={e => setTypeFilter(e.target.value)}
              className="w-full px-3 py-2 border border-slate-300 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-emerald-500">
              <option value="">All Types</option>
              {(tab === 'br' ? BR_TYPES : DR_TYPES).map(t => <option key={t} value={t}>{t}</option>)}
            </select>
          </div>
          <button onClick={() => handleSearch(1)} disabled={loading}
            className="px-5 py-2 bg-emerald-600 text-white text-sm font-semibold rounded-lg hover:bg-emerald-700 disabled:opacity-60 flex items-center gap-2">
            {loading ? <Loader2 size={15} className="animate-spin" /> : <Search size={15} />}
            Search
          </button>
        </div>
      </div>

      {error && (
        <div className="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded-xl text-sm">{error}</div>
      )}

      {/* Detail view */}
      {detailLoading && (
        <div className="flex items-center justify-center py-16 text-slate-400">
          <Loader2 size={22} className="animate-spin mr-2" /> Loading details…
        </div>
      )}

      {!detailLoading && brDetail && (
        <div className="bg-white border border-slate-200 rounded-xl p-5">
          <BRDetailView br={brDetail} onBack={() => setBrDetail(null)} />
        </div>
      )}

      {!detailLoading && drDetail && (
        <div className="bg-white border border-slate-200 rounded-xl p-5">
          <DRDetailView dr={drDetail} onBack={() => setDrDetail(null)} />
        </div>
      )}

      {/* Results */}
      {!showDetail && !detailLoading && searched && (
        <div className="bg-white border border-slate-200 rounded-xl overflow-hidden">
          <div className="px-5 py-3 border-b border-slate-100 flex items-center justify-between">
            <p className="text-sm font-semibold text-slate-700">
              {total > 0 ? `${total.toLocaleString()} record${total !== 1 ? 's' : ''} found` : 'No records found'}
            </p>
            {total > LIMIT && (
              <p className="text-xs text-slate-400">Page {page} of {totalPages}</p>
            )}
          </div>

          {results.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-16 text-slate-400">
              <FileX size={36} className="mb-3 opacity-40" />
              <p className="text-sm">No {tab.toUpperCase()} records match your search.</p>
            </div>
          ) : (
            <>
              <div className="overflow-x-auto">
                <table className="w-full text-sm text-left">
                  <thead className="bg-slate-50 text-xs text-slate-500 uppercase tracking-wider border-b border-slate-200">
                    <tr>
                      <th className="px-5 py-3">{tab === 'br' ? 'B.R. No.' : 'D.R. No.'}</th>
                      <th className="px-5 py-3">Date</th>
                      <th className="px-5 py-3">Type</th>
                      <th className="px-5 py-3">Passenger Name</th>
                      <th className="px-5 py-3">Passport No.</th>
                      <th className="px-5 py-3 text-right">{tab === 'br' ? 'Duty Paid' : 'Goods Value'}</th>
                      <th className="px-5 py-3">Links</th>
                      <th className="px-5 py-3 text-center">Action</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-slate-100">
                    {tab === 'br' && brResults.map((row) => (
                      <tr key={`${row.br_no}_${row.br_year}`} className="hover:bg-emerald-50/40 cursor-pointer" onClick={() => loadBrDetail(row)}>
                        <td className="px-5 py-3 font-mono font-semibold text-slate-800">{row.br_no}/{row.br_year ?? '—'}</td>
                        <td className="px-5 py-3 text-slate-600">{fmtDate(row.br_date)}</td>
                        <td className="px-5 py-3"><span className="px-2 py-0.5 rounded text-[11px] font-semibold bg-emerald-100 text-emerald-700">{row.br_type}</span></td>
                        <td className="px-5 py-3 text-slate-700">{row.pax_name || '—'}</td>
                        <td className="px-5 py-3 font-mono text-slate-600">{row.passport_no || '—'}</td>
                        <td className="px-5 py-3 text-right font-semibold text-emerald-700">{fmtRs(row.total_duty_paid)}</td>
                        <td className="px-5 py-3">
                          <div className="flex gap-1 flex-wrap">
                            {row.dr_no && <StatusBadge label={`DR:${row.dr_no}`} color="amber" />}
                            {row.os_no && <StatusBadge label={`OS:${row.os_no}`} color="purple" />}
                          </div>
                        </td>
                        <td className="px-5 py-3 text-center">
                          <button className="text-emerald-600 hover:text-emerald-800 flex items-center gap-1 text-xs font-medium mx-auto">
                            <ExternalLink size={13} /> View
                          </button>
                        </td>
                      </tr>
                    ))}
                    {tab === 'dr' && drResults.map((row) => (
                      <tr key={`${row.dr_no}_${row.dr_year}`} className="hover:bg-amber-50/40 cursor-pointer" onClick={() => loadDrDetail(row)}>
                        <td className="px-5 py-3 font-mono font-semibold text-slate-800">{row.dr_no}/{row.dr_year ?? '—'}</td>
                        <td className="px-5 py-3 text-slate-600">{fmtDate(row.dr_date)}</td>
                        <td className="px-5 py-3"><span className="px-2 py-0.5 rounded text-[11px] font-semibold bg-amber-100 text-amber-700">{row.dr_type}</span></td>
                        <td className="px-5 py-3 text-slate-700">{row.pax_name || '—'}</td>
                        <td className="px-5 py-3 font-mono text-slate-600">{row.passport_no || '—'}</td>
                        <td className="px-5 py-3 text-right font-semibold text-slate-700">{fmtRs(row.total_items_value)}</td>
                        <td className="px-5 py-3">
                          <div className="flex gap-1 flex-wrap">
                            <ClosureBadge ind={row.closure_ind} />
                            {row.os_no && <StatusBadge label={`OS:${row.os_no}`} color="purple" />}
                          </div>
                        </td>
                        <td className="px-5 py-3 text-center">
                          <button className="text-amber-600 hover:text-amber-800 flex items-center gap-1 text-xs font-medium mx-auto">
                            <ExternalLink size={13} /> View
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>

              {/* Pagination */}
              {totalPages > 1 && (
                <div className="px-5 py-3 border-t border-slate-100 flex items-center justify-between">
                  <p className="text-xs text-slate-400">{total.toLocaleString()} total · {LIMIT} per page</p>
                  <div className="flex gap-2">
                    <button onClick={() => handleSearch(page - 1)} disabled={page === 1}
                      className="p-1.5 rounded-lg border border-slate-200 text-slate-500 hover:bg-slate-50 disabled:opacity-40">
                      <ChevronLeft size={16} />
                    </button>
                    <span className="px-3 py-1.5 text-xs text-slate-600 font-medium">
                      {page} / {totalPages}
                    </span>
                    <button onClick={() => handleSearch(page + 1)} disabled={page === totalPages}
                      className="p-1.5 rounded-lg border border-slate-200 text-slate-500 hover:bg-slate-50 disabled:opacity-40">
                      <ChevronRight size={16} />
                    </button>
                  </div>
                </div>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
