import { useState, useMemo } from 'react';
import { useNavigate } from 'react-router-dom';
import { useQueryClient } from '@tanstack/react-query';
import { ShieldAlert, Search, Filter, RefreshCw, X } from 'lucide-react';
import { useOsList } from '@/hooks/useOsList';
import { useDebounce } from '@/hooks/useDebounce';

const PER_PAGE = 20;

const fmtDate = (d: string | null | undefined): string => {
  if (!d) return '—';
  const parts = d.split('-');
  if (parts.length === 3) return `${parts[2]}-${parts[1]}-${parts[0]}`;
  return d;
};

export default function QuashedList() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [currentPage, setCurrentPage] = useState(1);
  const [searchTerm, setSearchTerm] = useState('');
  const [showFilter, setShowFilter] = useState(false);
  const [filterYear, setFilterYear] = useState('');
  const debouncedSearch = useDebounce(searchTerm);

  const { data, isFetching } = useOsList({
    page: currentPage,
    per_page: PER_PAGE,
    search: debouncedSearch.trim(),
    status: 'quashed',
    ...(filterYear ? { year: filterYear } : {}),
  });

  const cases = data?.items ?? [];
  const total = data?.total ?? 0;
  const loading = isFetching;

  const handleSearchChange = (value: string) => { setSearchTerm(value); setCurrentPage(1); };
  const handlePageChange = (newPage: number) => setCurrentPage(newPage);

  const currentYear = new Date().getFullYear();
  const yearOptions = useMemo(() => Array.from({ length: currentYear - 1989 }, (_, i) => currentYear - i), [currentYear]);
  const totalPages = Math.ceil(total / PER_PAGE) || 1;
  const showing = { from: total === 0 ? 0 : (currentPage - 1) * PER_PAGE + 1, to: Math.min(currentPage * PER_PAGE, total) };

  return (
    <div className="space-y-6 flex flex-col max-w-7xl mx-auto pt-2 pb-12">
      {/* Header */}
      <div className="flex justify-between items-center bg-slate-700 text-white p-5 rounded-xl shadow-md border border-slate-600">
        <div>
          <h1 className="text-xl font-bold tracking-tight">Quashed &amp; Rejected Cases</h1>
          <p className="text-slate-300 text-sm mt-0.5">{total > 0 ? `${total.toLocaleString()} case(s) found` : 'No quashed or rejected cases.'}</p>
        </div>
        <button onClick={() => { setCurrentPage(1); queryClient.invalidateQueries({ queryKey: ['os', 'list'] }); }} className="flex items-center gap-2 bg-slate-600 hover:bg-slate-500 border border-slate-500 px-4 py-2 rounded-lg text-sm transition-colors">
          <RefreshCw size={15} className={loading ? 'animate-spin' : ''} /> Refresh
        </button>
      </div>

      {/* Search + Filter */}
      <div className="bg-white p-4 rounded-xl border border-slate-200 flex-shrink-0">
        <div className="flex flex-col md:flex-row gap-4 items-center justify-between">
          <div className="flex-1 w-full relative max-w-md">
            <div className="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
              <Search className="h-5 w-5 text-slate-400" />
            </div>
            <input type="text" className="w-full pl-10 pr-4 py-2 border border-slate-300 rounded-lg bg-slate-50 focus:bg-white focus:ring-2 focus:ring-slate-500 focus:border-slate-500 transition-colors text-sm"
              placeholder="Search by O.S. No, Pax Name, Passport..."
              value={searchTerm} onChange={e => handleSearchChange(e.target.value)} />
          </div>
          <button onClick={() => setShowFilter(f => !f)}
            className={`px-4 py-2 border rounded-lg font-medium flex items-center transition-colors text-sm ${filterYear ? 'border-slate-500 bg-slate-100 text-slate-700' : 'border-slate-300 bg-white text-slate-700 hover:bg-slate-50'}`}>
            <Filter size={16} className="mr-2" /> Filter
            {filterYear && <span className="ml-1.5 bg-slate-600 text-white text-xs rounded-full px-1.5 py-0.5 leading-none">1</span>}
          </button>
        </div>
        {showFilter && (
          <div className="mt-4 pt-4 border-t border-slate-200 flex flex-wrap gap-4 items-end">
            <div className="flex flex-col gap-1">
              <label className="text-xs font-semibold text-slate-500 uppercase tracking-wide">Year</label>
              <select value={filterYear} onChange={e => setFilterYear(e.target.value)}
                className="px-3 py-2 border border-slate-300 rounded-lg bg-slate-50 text-sm text-slate-700 focus:ring-2 focus:ring-slate-500">
                <option value="">All Years</option>
                {yearOptions.map(y => <option key={y} value={y}>{y}</option>)}
              </select>
            </div>
            <div className="flex gap-2">
              <button onClick={() => { setCurrentPage(1); setShowFilter(false); }}
                className="px-4 py-2 bg-slate-700 text-white font-medium rounded-lg hover:bg-slate-800 transition-colors text-sm">Apply</button>
              <button onClick={() => { setFilterYear(''); setCurrentPage(1); setShowFilter(false); }}
                className="px-4 py-2 border border-slate-300 bg-white text-slate-600 font-medium rounded-lg hover:bg-slate-50 transition-colors text-sm flex items-center">
                <X size={14} className="mr-1" /> Clear
              </button>
            </div>
          </div>
        )}
      </div>

      {/* Table */}
      <div className="bg-white rounded-xl flex-1 flex flex-col border border-slate-200">
        <div className="w-full relative">
          <table className="w-full text-sm text-left">
            <thead className="text-xs text-slate-700 uppercase bg-slate-100 border-b border-slate-200 sticky top-0 z-10">
              <tr>
                <th className="px-5 py-4 font-bold tracking-wider">O.S. Ref</th>
                <th className="px-5 py-4 font-bold tracking-wider">O/S Date</th>
                <th className="px-5 py-4 font-bold tracking-wider">Passenger</th>
                <th className="px-5 py-4 font-bold tracking-wider">Status</th>
                <th className="px-5 py-4 font-bold tracking-wider">Action By &amp; Date</th>
                <th className="px-5 py-4 font-bold tracking-wider">Reason</th>
                <th className="px-5 py-4 font-bold tracking-wider text-center w-20">View</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100">
              {loading ? (
                <tr><td colSpan={7} className="text-center py-12 text-slate-500">
                  <div className="flex flex-col items-center justify-center space-y-3">
                    <RefreshCw className="animate-spin text-slate-400" size={28} />
                    <span className="font-medium">Loading records...</span>
                  </div>
                </td></tr>
              ) : cases.length === 0 ? (
                <tr><td colSpan={7} className="text-center py-12 text-slate-500">
                  <div className="flex flex-col items-center justify-center space-y-2">
                    <ShieldAlert size={32} className="text-slate-300" />
                    <span className="font-medium">No quashed or rejected records found.</span>
                  </div>
                </td></tr>
              ) : cases.map((c, idx) => {
                const isQuashed = c.quashed === 'Y';
                const reasonText = isQuashed ? c.quash_reason : c.reject_reason;
                return (
                  <tr key={`${c.os_no}-${c.os_year}-${idx}`} onClick={() => navigate(`/adjudication/case/${c.os_no}/${c.os_year}`)}
                    className="hover:bg-slate-50 cursor-pointer group">
                    <td className="px-5 py-3 align-middle">
                      <div className="font-bold text-slate-700">{c.os_no}/{c.os_year}</div>
                      <div className="text-xs text-slate-400 mt-0.5">{c.location_code || 'CHN'}</div>
                      {c.is_offline_adjudication === 'Y' && (
                        <span className="inline-flex items-center mt-0.5 px-1.5 py-0.5 text-[10px] font-semibold rounded bg-purple-100 text-purple-700 border border-purple-200">
                          OFFLINE ADJ
                        </span>
                      )}
                    </td>
                    <td className="px-5 py-3 align-middle font-medium text-slate-600">{fmtDate(c.os_date)}</td>
                    <td className="px-5 py-3 align-middle">
                      <div className="font-bold text-slate-800">{c.pax_name || 'UNKNOWN'}</div>
                      <div className="text-xs text-slate-500 mt-0.5 font-mono">{c.passport_no || ''}</div>
                    </td>
                    <td className="px-5 py-3 align-middle">
                      <span className={`text-xs font-bold px-2 py-1 rounded-md ${isQuashed ? 'bg-red-100 text-red-800 border border-red-200' : 'bg-orange-100 text-orange-800 border border-orange-200'}`}>
                        {isQuashed ? 'QUASHED' : 'REJECTED'}
                      </span>
                    </td>
                    <td className="px-5 py-3 align-middle text-slate-700">
                      {c.quashed_by || '—'}
                      {isQuashed && c.quash_date && <span className="block text-xs text-slate-400">{fmtDate(c.quash_date)}</span>}
                    </td>
                    <td className="px-5 py-3 align-middle text-slate-600 text-xs max-w-xs truncate" title={reasonText}>{reasonText || '—'}</td>
                    <td className="px-5 py-3 align-middle text-center">
                      <span className="inline-flex items-center gap-1 bg-slate-200 text-slate-700 text-xs font-semibold px-3 py-1.5 rounded-lg group-hover:bg-slate-300 transition-colors">View</span>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
        <div className="bg-slate-50 border-t border-slate-200 p-4 flex justify-between items-center text-sm">
          <span className="text-slate-500 font-medium">
            {total === 0 ? 'No entries' : <>Showing <span className="text-slate-700">{showing.from}–{showing.to}</span> of <span className="text-slate-700">{total.toLocaleString()}</span> cases</>}
          </span>
          <div className="flex space-x-1">
            <button onClick={() => handlePageChange(currentPage - 1)} disabled={currentPage === 1 || loading}
              className="px-3 py-1.5 border border-slate-300 rounded-md bg-white text-slate-600 font-medium disabled:opacity-40 hover:bg-slate-50">Prev</button>
            <span className="px-3 py-1.5 border border-slate-500 bg-slate-100 text-slate-800 rounded-md font-bold">{currentPage} / {totalPages}</span>
            <button onClick={() => handlePageChange(currentPage + 1)} disabled={currentPage === totalPages || loading}
              className="px-3 py-1.5 border border-slate-300 rounded-md bg-white text-slate-600 font-medium disabled:opacity-40 hover:bg-slate-50">Next</button>
          </div>
        </div>
      </div>
    </div>
  );
}
