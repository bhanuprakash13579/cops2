import { useState, useMemo } from 'react';
import { useNavigate } from 'react-router-dom';
import { Gavel, Search, Filter, AlertCircle, RefreshCw, X, CheckCircle, FileDown, Loader2 } from 'lucide-react';
import { useQueryClient } from '@tanstack/react-query';
import { useOsList } from '@/hooks/useOsList';
import { useDebounce } from '@/hooks/useDebounce';
import api from '@/lib/api';
import { startDownload, progressDownload, completeDownload, failDownload } from '@/components/DownloadToast';

const PER_PAGE = 20;

const fmtDate = (d: string | null | undefined): string => {
  if (!d) return '—';
  const parts = d.split('-');
  if (parts.length === 3) return `${parts[2]}-${parts[1]}-${parts[0]}`;
  return d;
};

export default function AdjudicationList() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  const [currentPage, setCurrentPage] = useState(1);
  const [searchTerm, setSearchTerm] = useState('');
  const [showFilter, setShowFilter] = useState(false);
  const [filterYear, setFilterYear] = useState('');
  const [filterStatus, setFilterStatus] = useState('pending');

  const debouncedSearch = useDebounce(searchTerm);

  const { data, isFetching, error } = useOsList({
    page: currentPage,
    per_page: PER_PAGE,
    search: debouncedSearch,
    status: filterStatus || 'pending',
    ...(filterYear ? { year: filterYear } : {}),
  });

  const cases   = data?.items ?? [];
  const total   = data?.total ?? 0;
  const loading = isFetching && !data;
  const errorMsg = error ? (error as any).message ?? 'Failed to load cases' : '';

  const handleSearchChange = (value: string) => { setSearchTerm(value); setCurrentPage(1); };
  const handlePageChange   = (newPage: number) => setCurrentPage(newPage);
  const handleApplyFilter  = () => { setCurrentPage(1); setShowFilter(false); };
  const handleClearFilter  = () => {
    setFilterYear(''); setFilterStatus('pending'); setCurrentPage(1); setShowFilter(false);
  };

  const [downloadingKeys, setDownloadingKeys] = useState<Set<string>>(new Set());

  const handleDownload = async (os_no: string, os_year: number) => {
    const key = `${os_no}-${os_year}`;
    if (downloadingKeys.has(key)) return; // already in progress
    setDownloadingKeys(prev => new Set(prev).add(key));

    const label = `OS_${os_no}/${os_year}.pdf`;
    startDownload(key, label);

    // Fake progress: animate 0 → 70 % while server generates PDF (~binary search)
    let fakePct = 0;
    const fakeTimer = setInterval(() => {
      fakePct = Math.min(70, fakePct + 2);
      progressDownload(key, fakePct);
      if (fakePct >= 70) clearInterval(fakeTimer);
    }, 100);

    try {
      const res = await api.get(`/os/${os_no}/${os_year}/print-pdf`, {
        responseType: 'arraybuffer',
        onDownloadProgress: (evt) => {
          if (evt.total && evt.total > 0) {
            clearInterval(fakeTimer);
            // Map real transfer % onto 70–100 range
            progressDownload(key, 70 + Math.round((evt.loaded / evt.total) * 30));
          }
        },
      });
      clearInterval(fakeTimer);

      const defaultName = `OS_${os_no}_${os_year}.pdf`;
      let savedPath: string | undefined;
      try {
        const { save } = await import('@tauri-apps/plugin-dialog');
        const { writeFile } = await import('@tauri-apps/plugin-fs');
        const savePath = await save({
          title: 'Save OS as PDF',
          defaultPath: defaultName,
          filters: [{ name: 'PDF Files', extensions: ['pdf'] }],
        });
        if (savePath) {
          await writeFile(savePath, new Uint8Array(res.data));
          savedPath = savePath;
        }
      } catch {
        // Web fallback (non-Tauri)
        const blob = new Blob([res.data], { type: 'application/pdf' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url; a.download = defaultName;
        document.body.appendChild(a); a.click();
        document.body.removeChild(a);
        setTimeout(() => URL.revokeObjectURL(url), 5000);
      }
      completeDownload(key, savedPath);
    } catch {
      clearInterval(fakeTimer);
      failDownload(key);
    } finally {
      setDownloadingKeys(prev => { const s = new Set(prev); s.delete(key); return s; });
    }
  };

  const currentYear = new Date().getFullYear();
  const yearOptions = useMemo(() => Array.from({ length: currentYear - 1989 }, (_, i) => currentYear - i), [currentYear]);
  const totalPages = Math.ceil(total / PER_PAGE) || 1;
  const showing = { from: total === 0 ? 0 : (currentPage - 1) * PER_PAGE + 1, to: Math.min(currentPage * PER_PAGE, total) };
  const activeFilters = (filterYear ? 1 : 0) + (filterStatus && filterStatus !== 'pending' ? 1 : 0);

  return (
    <div className="space-y-6 flex flex-col max-w-7xl mx-auto pt-2 pb-12">
      {/* Header */}
      <div className="flex justify-between items-center bg-amber-800 text-white p-5 rounded-xl shadow-md border border-amber-700">
        <div>
          <h1 className="text-xl font-bold tracking-tight">Cases Pending Adjudication</h1>
          <p className="text-amber-200 text-sm mt-0.5">
            {total > 0 ? `${total.toLocaleString()} case(s) found` : 'No cases found.'}
          </p>
        </div>
        <button
          onClick={() => { setCurrentPage(1); queryClient.invalidateQueries({ queryKey: ['os', 'list'] }); }}
          className="flex items-center gap-2 bg-amber-700 hover:bg-amber-600 border border-amber-500 px-4 py-2 rounded-lg text-sm transition-colors"
        >
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
            <input
              type="text"
              className="w-full pl-10 pr-4 py-2 border border-slate-300 rounded-lg bg-slate-50 focus:bg-white focus:ring-2 focus:ring-amber-500 focus:border-amber-500 transition-colors text-sm"
              placeholder="Search by O.S. No, Pax Name, Passport or Flight..."
              value={searchTerm}
              onChange={e => handleSearchChange(e.target.value)}
            />
          </div>
          <button
            onClick={() => setShowFilter(f => !f)}
            className={`px-4 py-2 border rounded-lg font-medium flex items-center transition-colors text-sm ${activeFilters > 0 ? 'border-amber-400 bg-amber-50 text-amber-700 hover:bg-amber-100' : 'border-slate-300 bg-white text-slate-700 hover:bg-slate-50'}`}
          >
            <Filter size={16} className="mr-2" /> Filter
            {activeFilters > 0 && <span className="ml-1.5 bg-amber-600 text-white text-xs rounded-full px-1.5 py-0.5 leading-none">{activeFilters}</span>}
          </button>
        </div>
        {showFilter && (
          <div className="mt-4 pt-4 border-t border-slate-200">
            <div className="flex flex-wrap gap-4 items-end">
              <div className="flex flex-col gap-1">
                <label className="text-xs font-semibold text-slate-500 uppercase tracking-wide">Status</label>
                <select
                  value={filterStatus}
                  onChange={e => setFilterStatus(e.target.value)}
                  className="px-3 py-2 border border-slate-300 rounded-lg bg-slate-50 text-sm text-slate-700 focus:ring-2 focus:ring-amber-500 focus:border-amber-500"
                >
                  <option value="pending">Pending Adjudication</option>
                  <option value="adjudicated">Adjudicated</option>
                  <option value="">All Cases</option>
                </select>
              </div>
              <div className="flex flex-col gap-1">
                <label className="text-xs font-semibold text-slate-500 uppercase tracking-wide">Year</label>
                <select
                  value={filterYear}
                  onChange={e => setFilterYear(e.target.value)}
                  className="px-3 py-2 border border-slate-300 rounded-lg bg-slate-50 text-sm text-slate-700 focus:ring-2 focus:ring-amber-500 focus:border-amber-500"
                >
                  <option value="">All Years</option>
                  {yearOptions.map(y => <option key={y} value={y}>{y}</option>)}
                </select>
              </div>
              <div className="flex gap-2">
                <button onClick={handleApplyFilter} className="px-4 py-2 bg-amber-700 text-white font-medium rounded-lg hover:bg-amber-800 transition-colors text-sm">Apply</button>
                <button onClick={handleClearFilter} className="px-4 py-2 border border-slate-300 bg-white text-slate-600 font-medium rounded-lg hover:bg-slate-50 transition-colors text-sm flex items-center">
                  <X size={14} className="mr-1" /> Clear
                </button>
              </div>
            </div>
          </div>
        )}
      </div>

      {errorMsg && (
        <div className="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded-lg flex items-start">
          <AlertCircle className="shrink-0 mr-3 mt-0.5" size={20} />
          <p className="text-sm">{errorMsg}</p>
        </div>
      )}

      {/* Table */}
      <div className="bg-white rounded-xl flex-1 flex flex-col border border-slate-200">
        <div className="w-full relative">
          <table className="w-full text-sm text-left">
            <thead className="text-xs text-amber-800 uppercase bg-amber-50 border-b border-amber-200 sticky top-0 z-10">
              <tr>
                <th className="px-5 py-4 font-bold tracking-wider">O.S. Ref</th>
                <th className="px-5 py-4 font-bold tracking-wider">Date</th>
                <th className="px-5 py-4 font-bold tracking-wider">Passenger Name</th>
                <th className="px-5 py-4 font-bold tracking-wider">Flight / Passport</th>
                <th className="px-5 py-4 font-bold tracking-wider text-right">Value (₹)</th>
                <th className="px-5 py-4 font-bold tracking-wider text-center">Status</th>
                <th className="px-5 py-4 font-bold tracking-wider text-center w-32">Action</th>
                <th className="px-5 py-4 font-bold tracking-wider text-center w-24">PDF</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100">
              {loading ? (
                <tr>
                  <td colSpan={8} className="text-center py-12 text-slate-500">
                    <div className="flex flex-col items-center justify-center space-y-3">
                      <RefreshCw className="animate-spin text-amber-500" size={28} />
                      <span className="font-medium">Loading cases...</span>
                    </div>
                  </td>
                </tr>
              ) : cases.length === 0 ? (
                <tr>
                  <td colSpan={8} className="text-center py-12 text-slate-500">
                    <div className="flex flex-col items-center justify-center space-y-2">
                      <Gavel size={32} className="text-amber-200" />
                      <span className="font-medium">No cases found.</span>
                    </div>
                  </td>
                </tr>
              ) : (
                cases.map((row, idx) => {
                  // IMPORTANT: This must stay in sync with backend _pending_filters()
                  // in offence.py. The backend excludes cases where EITHER of these
                  // fields is set from the "pending" list. If you change this condition,
                  // update _pending_filters() too, or cases will appear in wrong lists.
                  const isAdjudicated = !!(row.adjudication_date || row.adj_offr_name);
                  return (
                    <tr key={`${row.os_no}-${row.os_year}-${idx}`} className="hover:bg-amber-50 group">
                      <td className="px-5 py-3 align-middle">
                        <div className="font-bold text-amber-800">{row.os_no}/{row.os_year}</div>
                        <div className="text-xs text-slate-400 mt-0.5">{row.location_code || 'CHN'}</div>
                        {row.is_offline_adjudication === 'Y' && (
                          <span className="inline-flex items-center mt-0.5 px-1.5 py-0.5 text-[10px] font-semibold rounded bg-purple-100 text-purple-700 border border-purple-200">
                            OFFLINE ADJ
                          </span>
                        )}
                      </td>
                      <td className="px-5 py-3 align-middle font-medium text-slate-600">{fmtDate(row.os_date)}</td>
                      <td className="px-5 py-3 align-middle">
                        <div className="font-bold text-slate-800">{row.pax_name || 'UNKNOWN'}</div>
                        <div className="text-xs text-slate-500 mt-0.5">{row.pax_nationality}</div>
                      </td>
                      <td className="px-5 py-3 align-middle">
                        <div className="text-slate-700 font-medium">{row.flight_no || 'N/A'}</div>
                        <div className="text-xs text-slate-500 mt-0.5 font-mono">{row.passport_no || 'N/A'}</div>
                      </td>
                      <td className="px-5 py-3 align-middle text-right">
                        <div className="font-bold text-slate-800">
                          {(row.total_items_value || 0).toLocaleString('en-IN', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
                        </div>
                        <div className="text-xs text-slate-400 mt-0.5">{row.total_items || 0} item(s)</div>
                      </td>
                      <td className="px-5 py-3 align-middle text-center">
                        {isAdjudicated ? (
                          <span className="inline-flex items-center gap-1 px-2.5 py-1 text-xs font-bold rounded-md border text-green-700 bg-green-50 border-green-200">
                            <CheckCircle size={11} /> ADJUDICATED
                          </span>
                        ) : (
                          <span className="inline-flex items-center px-2.5 py-1 text-xs font-bold rounded-md border text-orange-700 bg-orange-50 border-orange-200">
                            PENDING
                          </span>
                        )}
                      </td>
                      <td className="px-5 py-3 align-middle text-center">
                        {!isAdjudicated ? (
                          <button
                            onClick={() => navigate(`/adjudication/case/${row.os_no}/${row.os_year}`)}
                            className="inline-flex items-center gap-1.5 bg-amber-700 text-white text-xs font-semibold px-3 py-1.5 rounded-lg hover:bg-amber-600 transition-colors opacity-80 group-hover:opacity-100"
                          >
                            <Gavel size={13} /> Adjudicate
                          </button>
                        ) : (
                          <button
                            onClick={() => navigate(`/adjudication/case/${row.os_no}/${row.os_year}`)}
                            className="inline-flex items-center gap-1 bg-slate-200 text-slate-700 text-xs font-semibold px-3 py-1.5 rounded-lg hover:bg-slate-300 transition-colors"
                          >
                            View
                          </button>
                        )}
                      </td>
                      <td className="px-5 py-3 align-middle text-center">
                        {isAdjudicated && (() => {
                          const dlKey = `${row.os_no}-${row.os_year}`;
                          const isLoading = downloadingKeys.has(dlKey);
                          return (
                            <button
                              onClick={() => handleDownload(row.os_no, row.os_year)}
                              disabled={isLoading}
                              title="Download PDF"
                              className="inline-flex items-center gap-1 bg-emerald-600 text-white text-xs font-semibold px-3 py-1.5 rounded-lg hover:bg-emerald-700 disabled:opacity-50 transition-colors"
                            >
                              {isLoading ? <Loader2 size={13} className="animate-spin" /> : <FileDown size={13} />}
                              PDF
                            </button>
                          );
                        })()}
                      </td>
                    </tr>
                  );
                })
              )}
            </tbody>
          </table>
        </div>

        {/* Pagination */}
        <div className="bg-slate-50 border-t border-slate-200 p-4 flex justify-between items-center text-sm">
          <span className="text-slate-500 font-medium">
            {total === 0 ? 'No entries' : (
              <>Showing <span className="text-slate-700">{showing.from}–{showing.to}</span> of <span className="text-slate-700">{total.toLocaleString()}</span> cases</>
            )}
          </span>
          <div className="flex space-x-1">
            <button
              onClick={() => handlePageChange(currentPage - 1)}
              disabled={currentPage === 1 || loading}
              className="px-3 py-1.5 border border-slate-300 rounded-md bg-white text-slate-600 font-medium disabled:opacity-40 hover:bg-slate-50"
            >Prev</button>
            <span className="px-3 py-1.5 border border-amber-500 bg-amber-50 text-amber-800 rounded-md font-bold">
              {currentPage} / {totalPages}
            </span>
            <button
              onClick={() => handlePageChange(currentPage + 1)}
              disabled={currentPage === totalPages || loading}
              className="px-3 py-1.5 border border-slate-300 rounded-md bg-white text-slate-600 font-medium disabled:opacity-40 hover:bg-slate-50"
            >Next</button>
          </div>
        </div>
      </div>
    </div>
  );
}
