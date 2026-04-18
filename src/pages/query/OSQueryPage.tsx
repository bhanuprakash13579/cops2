import React, { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Search, Loader2, Printer, ChevronDown, ChevronUp, FileText, FileDown, ArrowUp, ArrowDown, ArrowUpDown } from 'lucide-react';
import api from '../../lib/api';
import DatePicker from '@/components/DatePicker';
import { showDownloadToast } from '@/components/DownloadToast';

// Page size for OS Query results. Kept at 50 to avoid scroll jank in
// Windows WebView2 — each row carries nested item data so 100+ rows
// cause a layout spike. Safe range: 20–80. Do NOT go above 100 without
// testing on the target Windows hardware.
const OS_QUERY_PAGE_SIZE = 50;

// Interface matching the backend OSQueryResponse schema
interface OSItem {
  items_sno: number;
  items_desc: string;
  items_qty: number;
  items_uqc: string;
  items_value: number;
  items_duty_type: string;
}

interface OSResult {
  os_no: string;
  os_year: number;
  os_date: string;
  pax_name: string;
  passport_no: string;
  flight_no: string;
  flight_date: string;
  total_items_value: number;
  total_duty_amount: number;
  total_payable: number;
  adjudication_date: string | null;
  is_draft: string;
  post_adj_br_entries: string | null;
  post_adj_dr_no: string | null;
  post_adj_dr_date: string | null;
  items: OSItem[];
  country_of_departure: string | null;
  item_desc_summary: string | null;
}

function SortIcon({ col, sortBy, sortDir }: { col: string; sortBy: string; sortDir: string }) {
  if (sortBy !== col) return <ArrowUpDown className="w-3 h-3 ml-1 opacity-30" />;
  return sortDir === 'desc'
    ? <ArrowDown className="w-3 h-3 ml-1 text-emerald-600" />
    : <ArrowUp className="w-3 h-3 ml-1 text-emerald-600" />;
}

export default function OSQueryPage() {
  const [loading, setLoading] = useState(false);
  const [results, setResults] = useState<OSResult[]>([]);
  const [pagination, setPagination] = useState({
    total_count: 0,
    page: 1,
    total_pages: 1,
    has_next: false,
    has_prev: false
  });
  const [hasSearched, setHasSearched] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [expandedRow, setExpandedRow] = useState<string | null>(null);
  const [sortBy, setSortBy] = useState<string>('os_year');
  const [sortDir, setSortDir] = useState<'asc' | 'desc'>('desc');
  const navigate = useNavigate();

  // Form State
  const [formData, setFormData] = useState({
    os_no: '',
    os_year: '',
    from_date: '',
    to_date: '',
    pax_name: '',
    passport_no: '',
    flight_no: '',
    country_of_departure: '',
    min_value: '',
    max_value: '',
    item_desc: '',
    case_type: ''
  });
  const [downloadLoading, setDownloadLoading] = useState(false);
  const [activeSearch, setActiveSearch] = useState<typeof formData>({ ...formData });

  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const { name, value } = e.target;
    setFormData(prev => ({ ...prev, [name]: value }));
  };

  const executeSearch = async (targetPage: number = 1, overrideSortBy?: string, overrideSortDir?: 'asc' | 'desc') => {
    setLoading(true);
    setHasSearched(false);
    setSearchError(null);
    setActiveSearch({ ...formData });

    // Clean up empty strings and parse numbers for the API
    const payload: Record<string, any> = {};
    Object.entries(formData).forEach(([key, value]) => {
      if (value.trim() !== '') {
        if (key === 'os_year') {
          payload[key] = parseInt(value, 10);
        } else if (key === 'min_value' || key === 'max_value') {
          payload[key] = parseFloat(value);
        } else {
          payload[key] = value.trim();
        }
      }
    });

    // Sort + Pagination params
    payload['sort_by'] = overrideSortBy ?? sortBy;
    payload['sort_dir'] = overrideSortDir ?? sortDir;
    payload['page'] = targetPage;
    payload['limit'] = OS_QUERY_PAGE_SIZE;

    try {
      const response = await api.post('/os-query/search', payload);
      setResults(response.data.items || []);
      setPagination({
        total_count: response.data.total_count || 0,
        page: response.data.page || 1,
        total_pages: response.data.total_pages || 1,
        has_next: response.data.has_next || false,
        has_prev: response.data.has_prev || false
      });
    } catch (err) {
      import.meta.env.DEV && console.error("Search failed:", err);
      setSearchError("Search failed. Please check your connection and try again.");
    } finally {
      setLoading(false);
      setHasSearched(true);
    }
  };

  const handleSearch = (e: React.FormEvent) => {
    e.preventDefault();
    executeSearch(1);
  };

  const toggleExpand = (id: string) => {
    setExpandedRow(prev => prev === id ? null : id);
  };

  const handleSort = (col: string) => {
    const newDir = sortBy === col && sortDir === 'desc' ? 'asc' : 'desc';
    setSortBy(col);
    setSortDir(newDir);
    // Re-fetch page 1 with new sort values passed directly (avoids stale closure)
    if (hasSearched) {
      executeSearch(1, col, newDir);
    }
  };


  // Build the search payload from the last executed search (activeSearch).
  const buildExportPayload = () => {
    const payload: Record<string, any> = { export: true, page: 1, limit: 5000 };
    Object.entries(activeSearch).forEach(([key, value]) => {
      if (value.trim() !== '') {
        if (key === 'os_year') payload[key] = parseInt(value, 10);
        else if (key === 'min_value' || key === 'max_value') payload[key] = parseFloat(value);
        else payload[key] = value.trim();
      }
    });
    payload['sort_by']  = sortBy;
    payload['sort_dir'] = sortDir;
    return payload;
  };

  const downloadCSV = async () => {
    setDownloadLoading(true);
    try {
      const response = await api.post('/os-query/search', buildExportPayload());
      const allRows: OSResult[] = response.data.items || [];

      const showCountry  = !!activeSearch.country_of_departure.trim();
      const showItemDesc = !!activeSearch.item_desc.trim();

      const headers = [
        'OS No', 'OS Year', 'OS Date', 'Passenger Name', 'Passport No', 'Flight No',
        ...(showCountry  ? ['Country of Dep'] : []),
        ...(showItemDesc ? ['Item Description'] : []),
        'Items (Desc × Qty Unit)',
        'Total Value (Rs)', 'Total Due (Rs)', 'Status', 'Adjudicated',
      ];

      const rows = allRows.map(r => {
        const itemsSummary = (r.items || [])
          .map(i => `${i.items_desc || ''}${i.items_qty != null ? ` x ${i.items_qty}` : ''}${i.items_uqc ? ` ${i.items_uqc}` : ''}`.trim())
          .join('; ');
        return [
          r.os_no, r.os_year, r.os_date,
          `"${(r.pax_name || '').replace(/"/g, '""')}"`,
          r.passport_no || '', r.flight_no || '',
          ...(showCountry  ? [`"${(r.country_of_departure || '').replace(/"/g, '""')}"`] : []),
          ...(showItemDesc ? [`"${(r.item_desc_summary    || '').replace(/"/g, '""')}"`] : []),
          `"${itemsSummary.replace(/"/g, '""')}"`,
          r.total_items_value || 0, r.total_payable || 0,
          r.is_draft === 'N' ? 'Submitted' : 'Draft',
          r.adjudication_date || 'No',
        ];
      });

      const csvString   = [headers.join(','), ...rows.map(e => e.join(','))].join('\n');
      const defaultName = `OS_Query_Results_${new Date().toISOString().split('T')[0]}.csv`;

      try {
        const { save }          = await import('@tauri-apps/plugin-dialog');
        const { writeTextFile } = await import('@tauri-apps/plugin-fs');
        const savePath = await save({ title: 'Save CSV', defaultPath: defaultName, filters: [{ name: 'CSV', extensions: ['csv'] }] });
        if (savePath) {
          await writeTextFile(savePath, csvString);
          showDownloadToast(`CSV saved to ${savePath} (${allRows.length} records)`);
        }
      } catch {
        const blob = new Blob([csvString], { type: 'text/csv' });
        const url  = URL.createObjectURL(blob);
        const a    = document.createElement('a');
        a.href = url; a.download = defaultName;
        document.body.appendChild(a); a.click(); document.body.removeChild(a);
        setTimeout(() => URL.revokeObjectURL(url), 5000);
        showDownloadToast(`CSV downloaded — ${allRows.length} records`);
      }
    } catch {
      import.meta.env.DEV && console.error('CSV export failed');
    } finally {
      setDownloadLoading(false);
    }
  };


  // Derived: which extra columns to show based on what was last searched
  const showCountry  = hasSearched && !!activeSearch.country_of_departure?.trim();
  const showItemDesc = hasSearched && !!activeSearch.item_desc?.trim();
  const totalCols    = 8 + (showCountry ? 1 : 0) + (showItemDesc ? 1 : 0); // +1 for always-visible Items column

  return (
    <div className="space-y-6 max-w-7xl mx-auto">
      <div className="bg-white rounded-xl shadow-sm border border-slate-200 overflow-hidden print:hidden">
        <div className="bg-slate-50 border-b border-slate-200 p-4">
          <h2 className="text-lg font-bold text-slate-800 flex items-center gap-2">
            <Search className="w-5 h-5 text-emerald-500" />
            Advanced OS Query
          </h2>
          <p className="text-sm text-slate-500 mt-1">Search legacy (.mdb) and new system records dynamically</p>
        </div>

        <form onSubmit={handleSearch} className="p-5">
          <div className="grid grid-cols-1 md:grid-cols-4 gap-4 mb-6">
            {/* Case Identifiers */}
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">OS No.</label>
              <input type="text" name="os_no" value={formData.os_no} onChange={handleInputChange} className="w-full bg-white border border-slate-300 shadow-sm rounded-md text-sm px-3 py-2 text-slate-800 placeholder-slate-400 focus:ring-emerald-500 focus:border-emerald-500 transition-shadow focus:shadow-md" placeholder="e.g. 142" />
            </div>
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">OS Year</label>
              <input type="number" name="os_year" value={formData.os_year} onChange={handleInputChange} className="w-full bg-white border border-slate-300 shadow-sm rounded-md text-sm px-3 py-2 text-slate-800 placeholder-slate-400 focus:ring-emerald-500 focus:border-emerald-500 transition-shadow focus:shadow-md" placeholder="e.g. 2023" />
            </div>
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">Flight No.</label>
              <input type="text" name="flight_no" value={formData.flight_no} onChange={handleInputChange} className="w-full bg-white border border-slate-300 shadow-sm rounded-md text-sm px-3 py-2 text-slate-800 placeholder-slate-400 focus:ring-emerald-500 focus:border-emerald-500 transition-shadow focus:shadow-md" placeholder="e.g. EK542" />
            </div>

            {/* Dates */}
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">From Date</label>
              <DatePicker value={formData.from_date} onChange={val => setFormData(f => ({ ...f, from_date: val }))} inputClassName="w-full bg-white border border-slate-300 rounded-md text-sm px-3 py-2 text-slate-800 placeholder-slate-400 focus:ring-emerald-500 focus:border-emerald-500" />
            </div>
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">To Date</label>
              <DatePicker value={formData.to_date} onChange={val => setFormData(f => ({ ...f, to_date: val }))} inputClassName="w-full bg-white border border-slate-300 rounded-md text-sm px-3 py-2 text-slate-800 placeholder-slate-400 focus:ring-emerald-500 focus:border-emerald-500" />
            </div>

            {/* Pax Info */}
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">Passenger Name</label>
              <input type="text" name="pax_name" value={formData.pax_name} onChange={handleInputChange} className="w-full bg-white border border-slate-300 shadow-sm rounded-md text-sm px-3 py-2 text-slate-800 placeholder-slate-400 focus:ring-emerald-500 focus:border-emerald-500 transition-shadow focus:shadow-md" placeholder="Full or partial name" />
            </div>
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">Passport No.</label>
              <input type="text" name="passport_no" value={formData.passport_no} onChange={handleInputChange} className="w-full bg-white border border-slate-300 shadow-sm rounded-md text-sm px-3 py-2 text-slate-800 placeholder-slate-400 focus:ring-emerald-500 focus:border-emerald-500 transition-shadow focus:shadow-md" placeholder="e.g. L1234567" />
            </div>

            {/* Flight/Route */}
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">Country of Dep (Arrived From)</label>
              <input type="text" name="country_of_departure" value={formData.country_of_departure} onChange={handleInputChange} className="w-full bg-white border border-slate-300 shadow-sm rounded-md text-sm px-3 py-2 text-slate-800 placeholder-slate-400 focus:ring-emerald-500 focus:border-emerald-500 transition-shadow focus:shadow-md" placeholder="e.g. DUBAI" />
            </div>

            {/* Goods / Value */}
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">Item Description</label>
              <input type="text" name="item_desc" value={formData.item_desc} onChange={handleInputChange} className="w-full bg-white border border-slate-300 shadow-sm rounded-md text-sm px-3 py-2 text-slate-800 placeholder-slate-400 focus:ring-emerald-500 focus:border-emerald-500 transition-shadow focus:shadow-md" placeholder="e.g. Gold" />
            </div>
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">Min Value (₹)</label>
              <input type="number" name="min_value" value={formData.min_value} onChange={handleInputChange} className="w-full bg-white border border-slate-300 shadow-sm rounded-md text-sm px-3 py-2 text-slate-800 placeholder-slate-400 focus:ring-emerald-500 focus:border-emerald-500 transition-shadow focus:shadow-md" placeholder="Min value..." />
            </div>
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">Max Value (₹)</label>
              <input type="number" name="max_value" value={formData.max_value} onChange={handleInputChange} className="w-full bg-white border border-slate-300 shadow-sm rounded-md text-sm px-3 py-2 text-slate-800 placeholder-slate-400 focus:ring-emerald-500 focus:border-emerald-500 transition-shadow focus:shadow-md" placeholder="Max value..." />
            </div>
            <div>
              <label className="block text-xs font-semibold text-slate-600 mb-1">Case Type</label>
              <select name="case_type" value={formData.case_type} onChange={e => setFormData(f => ({ ...f, case_type: e.target.value }))} className="w-full bg-white border border-slate-300 shadow-sm rounded-md text-sm px-3 py-2 text-slate-800 focus:ring-emerald-500 focus:border-emerald-500">
                <option value="">All Cases</option>
                <option value="Arrival Case">Arrival Cases</option>
                <option value="Export Case">Export Cases</option>
              </select>
            </div>
          </div>

          <div className="flex justify-end border-t border-slate-100 pt-4">
            <button 
              type="button" 
              onClick={() => setFormData({os_no:'', os_year:'', from_date:'', to_date:'', pax_name:'', passport_no:'', flight_no:'', country_of_departure:'', min_value:'', max_value:'', item_desc:'', case_type:''})}
              className="px-4 py-2 text-sm font-medium text-slate-600 bg-white border border-slate-300 shadow-sm rounded-md hover:bg-slate-50 mr-3"
            >
              Clear
            </button>
            <button 
              type="submit" 
              disabled={loading}
              className="flex items-center gap-2 px-6 py-2 text-sm font-medium text-white bg-emerald-600 rounded-md hover:bg-emerald-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-emerald-500 disabled:opacity-50"
            >
              {loading ? <Loader2 className="w-4 h-4 animate-spin" /> : <Search className="w-4 h-4" />}
              Search Records
            </button>
          </div>
        </form>
      </div>


      {/* Inline search error */}
      {searchError && (
        <div className="bg-red-50 border border-red-200 text-red-700 text-sm rounded-lg px-4 py-3 flex items-center gap-2">
          <span className="font-semibold">Error:</span> {searchError}
        </div>
      )}

      {/* Results Table */}
      {hasSearched && !searchError && (
        <div className="bg-white rounded-xl shadow-sm border border-slate-200 overflow-hidden flex flex-col items-stretch print:border-none print:shadow-none">
          <div className="hidden print:block mb-4">
            <h1 className="text-xl font-bold border-b pb-2">Customs OS Query Report</h1>
            <p className="text-sm mt-1 text-slate-600">Generated on: {new Date().toLocaleDateString()}</p>
          </div>
          
          <div className="bg-slate-50 border-b border-slate-200 p-4 flex justify-between items-center print:hidden">
            <h3 className="font-bold text-slate-800">
              Search Results
              <span className="ml-2 text-xs font-medium bg-emerald-100 text-emerald-800 px-2 py-0.5 rounded-full">
                {results.length} record{results.length !== 1 && 's'} showing (Total: {pagination.total_count})
              </span>
            </h3>
            {results.length > 0 && (
              <div className="flex items-center gap-2">
                <button
                  onClick={downloadCSV}
                  disabled={downloadLoading}
                  className="flex items-center gap-1.5 text-xs font-medium text-white bg-emerald-600 border border-emerald-600 px-3 py-1.5 rounded hover:bg-emerald-700 transition-colors shadow-sm disabled:opacity-50"
                >
                  {downloadLoading ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <FileDown className="w-3.5 h-3.5" />}
                  Download CSV ({pagination.total_count})
                </button>
              </div>
            )}
          </div>

          {results.length === 0 ? (
            <div className="p-12 text-center text-slate-500">
              <FileText className="w-12 h-12 mx-auto text-slate-300 mb-3" />
              <p>No records found matching your query criteria.</p>
            </div>
          ) : (
            <div className="overflow-auto">
              <table className="min-w-full divide-y divide-slate-200">
                <thead className="bg-slate-50">
                  <tr>
                    <th className="px-4 py-3 text-left text-xs font-semibold text-slate-600 uppercase tracking-wider">Expand</th>
                    <th className="px-4 py-3 text-left text-xs font-semibold text-slate-600 uppercase tracking-wider">
                      <button onClick={() => handleSort('os_year')} className="flex items-center hover:text-slate-900 transition-colors">
                        OS Details <SortIcon col="os_year" sortBy={sortBy} sortDir={sortDir} />
                      </button>
                    </th>
                    <th className="px-4 py-3 text-left text-xs font-semibold text-slate-600 uppercase tracking-wider">
                      <button onClick={() => handleSort('pax_name')} className="flex items-center hover:text-slate-900 transition-colors">
                        Passenger <SortIcon col="pax_name" sortBy={sortBy} sortDir={sortDir} />
                      </button>
                    </th>
                    <th className="px-4 py-3 text-left text-xs font-semibold text-slate-600 uppercase tracking-wider">
                      <button onClick={() => handleSort('flight_date')} className="flex items-center hover:text-slate-900 transition-colors">
                        Flight <SortIcon col="flight_date" sortBy={sortBy} sortDir={sortDir} />
                      </button>
                    </th>
                    {showCountry && (
                      <th className="px-4 py-3 text-left text-xs font-semibold text-slate-600 uppercase tracking-wider">Country of Dep</th>
                    )}
                    {showItemDesc && (
                      <th className="px-4 py-3 text-left text-xs font-semibold text-slate-600 uppercase tracking-wider">Item Description</th>
                    )}
                    <th className="px-4 py-3 text-left text-xs font-semibold text-amber-700 uppercase tracking-wider bg-amber-50">Items</th>
                    <th className="px-4 py-3 text-right text-xs font-semibold text-slate-600 uppercase tracking-wider">
                      <button onClick={() => handleSort('total_items_value')} className="flex items-center ml-auto hover:text-slate-900 transition-colors">
                        Values (₹) <SortIcon col="total_items_value" sortBy={sortBy} sortDir={sortDir} />
                      </button>
                    </th>
                    <th className="px-4 py-3 text-center text-xs font-semibold text-slate-600 uppercase tracking-wider">
                      <button onClick={() => handleSort('adjudication_date')} className="flex items-center mx-auto hover:text-slate-900 transition-colors">
                        Status <SortIcon col="adjudication_date" sortBy={sortBy} sortDir={sortDir} />
                      </button>
                    </th>
                    <th className="px-4 py-3 text-center text-xs font-semibold text-slate-600 uppercase tracking-wider print:hidden">Action</th>
                  </tr>
                </thead>
                <tbody className="bg-white divide-y divide-slate-100">
                  {results.map((r, idx) => {
                    const rowId = `${r.os_no}-${r.os_year}`;
                    const isExpanded = expandedRow === rowId;
                    return (
                      <React.Fragment key={idx}>
                        <tr className="hover:bg-slate-50 transition-colors break-inside-avoid">
                          <td className="px-4 py-3 whitespace-nowrap print:hidden">
                            <button onClick={() => toggleExpand(rowId)} className="p-1 rounded text-slate-400 hover:bg-slate-200 hover:text-slate-600">
                              {isExpanded ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
                            </button>
                          </td>
                          <td className="px-4 py-3">
                            <div className="font-bold text-slate-800 text-sm">OS {r.os_no}/{r.os_year}</div>
                            <div className="text-xs text-slate-500">{r.os_date}</div>
                          </td>
                          <td className="px-4 py-3">
                            <div className="font-medium text-slate-800 text-sm truncate max-w-[200px]">{r.pax_name || 'N/A'}</div>
                            <div className="text-xs text-slate-500">PP: {r.passport_no || 'N/A'}</div>
                          </td>
                          <td className="px-4 py-3">
                            <div className="text-sm text-slate-800">{r.flight_no || 'N/A'}</div>
                            <div className="text-xs text-slate-500">{r.flight_date || ''}</div>
                          </td>
                          {showCountry && (
                            <td className="px-4 py-3">
                              <div className="text-sm text-slate-800 truncate max-w-[130px]">{r.country_of_departure || '—'}</div>
                            </td>
                          )}
                          {showItemDesc && (
                            <td className="px-4 py-3">
                              <div className="text-sm text-slate-700 truncate max-w-[200px]" title={r.item_desc_summary || ''}>{r.item_desc_summary || '—'}</div>
                            </td>
                          )}
                          <td className="px-4 py-2 align-top bg-amber-50/30">
                            {(r.items || []).length === 0 ? (
                              <span className="text-xs text-slate-400">—</span>
                            ) : (
                              <div className="space-y-0.5">
                                {(r.items || []).map((item, ii) => (
                                  <div key={ii} className="text-xs leading-tight">
                                    <span className="font-medium text-slate-800">{item.items_desc || '—'}</span>
                                    {(item.items_qty != null || item.items_uqc) && (
                                      <span className="text-slate-500 ml-1">× {item.items_qty} {item.items_uqc}</span>
                                    )}
                                  </div>
                                ))}
                              </div>
                            )}
                          </td>
                          <td className="px-4 py-3 text-right">
                            <div className="text-sm font-semibold text-slate-800">
                              Val: {r.total_items_value?.toLocaleString('en-IN') || '0'}
                            </div>
                            <div className="text-xs text-rose-600">
                              Due: {r.total_payable?.toLocaleString('en-IN') || '0'}
                            </div>
                          </td>
                          <td className="px-4 py-3 text-center whitespace-nowrap">
                            {r.is_draft === 'N' ? (
                              <span className="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-emerald-100 text-emerald-800">
                                Submitted
                              </span>
                            ) : (
                              <span className="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-amber-100 text-amber-800">
                                Draft
                              </span>
                            )}
                            {r.adjudication_date && (
                              <div className="text-[10px] text-slate-500 mt-1">Adjudicated</div>
                            )}
                          </td>
                          <td className="px-4 py-3 text-center whitespace-nowrap print:hidden">
                            <button
                              onClick={() => navigate(`/query/os/print/${r.os_no}/${r.os_year}`)}
                              className="inline-flex items-center gap-1.5 px-3 py-1.5 bg-emerald-50 text-emerald-700 hover:bg-emerald-100 hover:text-emerald-800 rounded-md text-xs font-semibold transition-colors border border-emerald-200"
                              title="View & Print Full Report"
                            >
                              <Printer className="w-3 h-3" /> Print
                            </button>
                          </td>
                        </tr>
                        
                        {/* Expandable row for Items */}
                        {isExpanded && (
                          <tr className="bg-slate-50/80 border-b border-slate-200">
                            <td colSpan={totalCols} className="px-8 py-4">
                              <div className="border border-slate-200 rounded bg-white shadow-sm font-mono text-xs overflow-hidden">
                                <div className="bg-slate-100 border-b border-slate-200 px-3 py-1.5 font-bold text-slate-600 text-[10px] uppercase tracking-wider flex justify-between">
                                  <span>Seized Goods / Items Inventory</span>
                                  <span>{(r.items || []).length} item(s)</span>
                                </div>
                                {(r.items || []).length === 0 ? (
                                  <div className="p-3 text-slate-400 italic text-center">No items recorded</div>
                                ) : (
                                  <table className="min-w-full divide-y divide-slate-100 table-fixed">
                                    <thead className="bg-slate-50">
                                      <tr>
                                        <th className="w-10 px-3 py-2 text-left text-[10px] text-slate-500">SNo</th>
                                        <th className="px-3 py-2 text-left text-[10px] text-slate-500 uppercase">Description</th>
                                        <th className="w-24 px-3 py-2 text-right text-[10px] text-slate-500 uppercase">Qty/UQC</th>
                                        <th className="w-28 px-3 py-2 text-right text-[10px] text-slate-500 uppercase">Value (₹)</th>
                                      </tr>
                                    </thead>
                                    <tbody className="divide-y divide-slate-50">
                                      {(r.items || []).map((item, i) => (
                                        <tr key={i} className={i % 2 === 0 ? 'bg-white' : 'bg-slate-50/30'}>
                                          <td className="px-3 py-2 text-slate-500">{item.items_sno}</td>
                                          <td className="px-3 py-2 text-slate-800 truncate" title={item.items_desc}>{item.items_desc || '—'}</td>
                                          <td className="px-3 py-2 text-slate-700 text-right">{item.items_qty} {item.items_uqc}</td>
                                          <td className="px-3 py-2 text-slate-800 font-medium text-right">{item.items_value.toLocaleString('en-IN')}</td>
                                        </tr>
                                      ))}
                                    </tbody>
                                  </table>
                                )}
                              </div>

                              {/* Post-Adjudication BR/DR metadata */}
                              {(r.post_adj_br_entries || r.post_adj_dr_no) && (
                                <div className="mt-3 border border-amber-200 rounded bg-amber-50/50 overflow-hidden">
                                  <div className="bg-amber-100 border-b border-amber-200 px-3 py-1.5 font-bold text-amber-800 text-[10px] uppercase tracking-wider">
                                    Post-Adjudication Receipts
                                  </div>
                                  <div className="px-3 py-2 text-xs text-slate-700 space-y-1 font-sans">
                                    {r.post_adj_br_entries && (() => {
                                      try {
                                        const brs: { no: string; date: string | null }[] = JSON.parse(r.post_adj_br_entries);
                                        return brs.map((b, i) => (
                                          <div key={i}>
                                            <span className="font-semibold text-amber-800">BR No.:</span>{' '}
                                            {b.no}
                                            {b.date && <span className="text-slate-500 ml-2">({b.date.split('-').reverse().join('-')})</span>}
                                          </div>
                                        ));
                                      } catch { return null; }
                                    })()}
                                    {r.post_adj_dr_no && (
                                      <div>
                                        <span className="font-semibold text-amber-800">DR No.:</span>{' '}
                                        {r.post_adj_dr_no}
                                        {r.post_adj_dr_date && (
                                          <span className="text-slate-500 ml-2">({r.post_adj_dr_date.split('-').reverse().join('-')})</span>
                                        )}
                                      </div>
                                    )}
                                  </div>
                                </div>
                              )}
                            </td>
                          </tr>
                        )}
                      </React.Fragment>
                    );
                  })}
                </tbody>
              </table>
              
              {/* Pagination Controls */}
              {pagination.total_pages > 1 && (
                <div className="flex items-center justify-between px-4 py-3 bg-white border-t border-slate-200 sm:px-6 print:hidden">
                  <div className="flex flex-1 justify-between sm:hidden">
                    <button
                      onClick={() => executeSearch(pagination.page - 1, sortBy, sortDir)}
                      disabled={!pagination.has_prev}
                      className="relative inline-flex items-center rounded-md border border-slate-300 bg-white px-4 py-2 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-50"
                    >
                      Previous
                    </button>
                    <button
                      onClick={() => executeSearch(pagination.page + 1, sortBy, sortDir)}
                      disabled={!pagination.has_next}
                      className="relative ml-3 inline-flex items-center rounded-md border border-slate-300 bg-white px-4 py-2 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-50"
                    >
                      Next
                    </button>
                  </div>
                  <div className="hidden sm:flex sm:flex-1 sm:items-center sm:justify-between">
                    <div>
                      <p className="text-sm text-slate-700">
                        Showing page <span className="font-medium">{pagination.page}</span> of <span className="font-medium">{pagination.total_pages}</span>
                      </p>
                    </div>
                    <div>
                      <nav className="isolate inline-flex -space-x-px rounded-md shadow-sm" aria-label="Pagination">
                        <button
                          onClick={() => executeSearch(pagination.page - 1, sortBy, sortDir)}
                          disabled={!pagination.has_prev}
                          className="relative inline-flex items-center rounded-l-md px-2 py-2 text-slate-400 ring-1 ring-inset ring-slate-300 hover:bg-slate-50 focus:z-20 focus:outline-offset-0 disabled:opacity-50"
                        >
                          <span className="sr-only">Previous</span>
                          <span className="text-sm font-medium px-2">Previous</span>
                        </button>
                        <button
                          onClick={() => executeSearch(pagination.page + 1, sortBy, sortDir)}
                          disabled={!pagination.has_next}
                          className="relative inline-flex items-center rounded-r-md px-2 py-2 text-slate-400 ring-1 ring-inset ring-slate-300 hover:bg-slate-50 focus:z-20 focus:outline-offset-0 disabled:opacity-50"
                        >
                          <span className="sr-only">Next</span>
                          <span className="text-sm font-medium px-2">Next</span>
                        </button>
                      </nav>
                    </div>
                  </div>
                </div>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
