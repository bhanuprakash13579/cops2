import { useState, Component, ReactNode } from 'react';
import { Routes, Route, useNavigate, Navigate } from 'react-router-dom';
import { FileSearch, LogOut, Menu, ChevronLeft, Download, FileText, Users, AlertTriangle, BarChart2, Receipt } from 'lucide-react';
import { useAuth } from '../../contexts/AuthContext';
import OSQueryPage from './OSQueryPage';
import OSPrintView from './OSPrintView';
import ExportData from './ExportData';
import CustomReport from './CustomReport';
import AdjudicationSummaryReport from './AdjudicationSummaryReport';
import MonthlyReportPage from './MonthlyReportPage';
import BRDRLookupPage from './BRDRLookupPage';

class QueryErrorBoundary extends Component<{ children: ReactNode }, { error: string | null }> {
  constructor(props: { children: ReactNode }) {
    super(props);
    this.state = { error: null };
  }
  static getDerivedStateFromError(err: Error) {
    return { error: err.message || 'Unknown error' };
  }
  render() {
    if (this.state.error) {
      return (
        <div className="flex flex-col items-center justify-center h-64 gap-3 text-slate-500">
          <AlertTriangle size={32} className="text-amber-500" />
          <p className="text-sm font-semibold text-slate-700">Something went wrong in this view.</p>
          <p className="text-xs text-red-600 max-w-md text-center font-mono">{this.state.error}</p>
          <button
            className="mt-2 px-4 py-2 text-xs rounded-lg bg-slate-700 text-white hover:bg-slate-800"
            onClick={() => this.setState({ error: null })}
          >
            Try again
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

export default function QueryModule() {
  const { user, logout } = useAuth();
  const navigate = useNavigate();

  const [isCollapsed, setIsCollapsed] = useState(true);

  const handleLogout = () => {
    logout();
    navigate('/modules');
  };

  return (
    <div className="flex min-h-screen w-full bg-slate-50 text-slate-800 print:h-auto print:min-h-0 print:overflow-visible print:block print-layout-root">
      {/* Sidebar - hidden when printing */}
      <aside className={`${isCollapsed ? 'w-16' : 'w-64'} flex flex-col bg-slate-900 border-r border-slate-700 shadow-xl shadow-slate-900/20 transition-[width] duration-300 ease-in-out print:hidden shrink-0 z-50 sticky top-0 h-screen overflow-hidden`}>
        <div className="p-3 border-b border-slate-700/50 flex items-center justify-between min-h-[72px] shrink-0">
            <div
              className="flex items-center gap-3 transition-[opacity,transform] duration-300 ease-in-out"
              style={{ opacity: isCollapsed ? 0 : 1, transform: isCollapsed ? 'translateX(-8px)' : 'translateX(0)', width: isCollapsed ? 0 : 'auto', overflow: 'hidden', pointerEvents: isCollapsed ? 'none' : 'auto' }}
            >
                <div className="bg-gradient-to-br from-emerald-500 to-emerald-700 p-2 rounded-lg shrink-0">
                  <FileSearch className="text-white w-6 h-6" />
                </div>
                <div className="flex-1 min-w-0 whitespace-nowrap">
                  <h1 className="text-white font-bold text-base tracking-wide leading-tight">Query Module</h1>
                  <p className="text-emerald-400 text-xs">COPS</p>
                </div>
            </div>
            
            <button
              type="button"
              onClick={() => setIsCollapsed(v => !v)}
              className={`text-slate-400 hover:text-white hover:bg-slate-800 p-2 rounded-lg transition-colors ${isCollapsed ? 'mx-auto' : 'ml-1 shrink-0'}`}
              title={isCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
            >
              <Menu size={26} />
            </button>
          </div>
        
        <div className="flex-1 overflow-y-auto overflow-x-hidden overscroll-contain min-h-0">
          <nav className="p-3 space-y-1 mt-2">
            {!isCollapsed && <p className="text-emerald-500/70 text-xs uppercase tracking-widest font-semibold px-3 mb-3 whitespace-nowrap transition-opacity duration-300" style={{ opacity: isCollapsed ? 0 : 1 }}>Search & Reports</p>}
            
            <button
              onClick={() => navigate('/query/os')}
              className={`w-full flex items-center ${isCollapsed ? 'justify-center py-3' : 'gap-3 px-4 py-3'} rounded-lg text-base transition-colors text-emerald-400 bg-emerald-500/15 border border-emerald-500/20 hover:bg-emerald-500/25`}
              title={isCollapsed ? 'OS Query' : undefined}
            >
              <FileSearch className="w-6 h-6 shrink-0 opacity-90" />
              {!isCollapsed && <span className="font-medium leading-tight">OS Query</span>}
            </button>

            <button
              onClick={() => navigate('/query/report')}
              className={`mt-2 w-full flex items-center ${isCollapsed ? 'justify-center py-3' : 'gap-3 px-4 py-3'} rounded-lg text-base transition-colors text-emerald-200 bg-slate-900/40 border border-slate-700/60 hover:bg-slate-800/70`}
              title={isCollapsed ? 'Custom Report' : undefined}
            >
              <FileText className="w-5 h-5 shrink-0 opacity-90" />
              {!isCollapsed && <span className="font-medium leading-tight">Custom Report</span>}
            </button>

            <button
              onClick={() => navigate('/query/export')}
              className={`mt-2 w-full flex items-center ${isCollapsed ? 'justify-center py-3' : 'gap-3 px-4 py-3'} rounded-lg text-base transition-colors text-emerald-200 bg-slate-900/40 border border-slate-700/60 hover:bg-slate-800/70`}
              title={isCollapsed ? 'Download Backup' : undefined}
            >
              <Download className="w-5 h-5 shrink-0 opacity-90" />
              {!isCollapsed && <span className="font-medium leading-tight">Download Backup</span>}
            </button>

            <button
              onClick={() => navigate('/query/adjn-summary')}
              className={`mt-2 w-full flex items-center ${isCollapsed ? 'justify-center py-3' : 'gap-3 px-4 py-3'} rounded-lg text-base transition-colors text-emerald-200 bg-slate-900/40 border border-slate-700/60 hover:bg-slate-800/70`}
              title={isCollapsed ? 'Officer Summary' : undefined}
            >
              <Users className="w-5 h-5 shrink-0 opacity-90" />
              {!isCollapsed && <span className="font-medium leading-tight">Officer Summary</span>}
            </button>

            <button
              onClick={() => navigate('/query/monthly-report')}
              className={`mt-2 w-full flex items-center ${isCollapsed ? 'justify-center py-3' : 'gap-3 px-4 py-3'} rounded-lg text-base transition-colors text-emerald-200 bg-slate-900/40 border border-slate-700/60 hover:bg-slate-800/70`}
              title={isCollapsed ? 'Monthly Report' : undefined}
            >
              <BarChart2 className="w-5 h-5 shrink-0 opacity-90" />
              {!isCollapsed && <span className="font-medium leading-tight">Monthly Report</span>}
            </button>

            <button
              onClick={() => navigate('/query/br-dr')}
              className={`mt-2 w-full flex items-center ${isCollapsed ? 'justify-center py-3' : 'gap-3 px-4 py-3'} rounded-lg text-base transition-colors text-emerald-200 bg-slate-900/40 border border-slate-700/60 hover:bg-slate-800/70`}
              title={isCollapsed ? 'BR / DR Lookup' : undefined}
            >
              <Receipt className="w-5 h-5 shrink-0 opacity-90" />
              {!isCollapsed && <span className="font-medium leading-tight">BR / DR Lookup</span>}
            </button>
          </nav>
        </div>
        
        <div className={`border-t border-slate-700/50 space-y-3 shrink-0 ${isCollapsed ? 'p-2' : 'p-4'}`}>
          <div
            className="bg-slate-800 rounded-lg border border-slate-700/50 transition-[opacity,max-height] duration-300 ease-in-out overflow-hidden"
            style={{ opacity: isCollapsed ? 0 : 1, maxHeight: isCollapsed ? 0 : '120px', padding: isCollapsed ? 0 : '0.75rem', borderColor: isCollapsed ? 'transparent' : undefined }}
          >
              <div className="flex items-center gap-2 mb-1 whitespace-nowrap">
                <FileSearch size={18} className="text-emerald-400" />
                <p className="text-emerald-300 text-xs uppercase tracking-wider font-semibold">Logged in as</p>
              </div>
              <p className="text-white font-semibold text-sm truncate">{user?.user_name}</p>
              <p className="text-emerald-400 text-xs">{user?.user_desig || user?.user_role}</p>
          </div>
          
          <button
            onClick={() => navigate('/modules')}
            className={`flex w-full items-center rounded-lg text-slate-400 hover:bg-slate-800 hover:text-white transition-colors text-base ${isCollapsed ? 'justify-center py-3' : 'gap-2 px-4 py-3'}`}
            title="Module Selection"
          >
            <ChevronLeft size={22} />
            {!isCollapsed && 'Module Selection'}
          </button>
          
          <button
            onClick={handleLogout}
            className={`flex w-full items-center rounded-lg text-slate-400 hover:bg-rose-500/10 hover:text-rose-400 transition-colors text-base border border-transparent hover:border-rose-500/20 ${isCollapsed ? 'justify-center py-3' : 'gap-2 px-4 py-3'}`}
            title="Sign Out"
          >
            <LogOut size={22} />
            {!isCollapsed && 'Sign Out'}
          </button>
        </div>
      </aside>

      {/* Main Content */}
      <main className="flex-1 flex flex-col min-h-screen bg-slate-50/50 print:bg-white relative print:h-auto print:min-h-0 print:overflow-visible print:block">
        <div className="h-[2px] w-full bg-gradient-to-r from-emerald-500 via-emerald-400 to-transparent fixed top-0 z-50 print:hidden print-hide-bar" data-print-hide="true"></div>
        <div className="flex-1 min-h-0 p-4 md:p-8 pt-10 print:p-0 print:overflow-visible print-content-wrap">
          <QueryErrorBoundary>
            <Routes>
              <Route path="/" element={<Navigate to="/query/os" replace />} />
              <Route path="os" element={<OSQueryPage />} />
              <Route path="os/print/:os_no/:os_year" element={<OSPrintView />} />
              <Route path="report" element={<CustomReport />} />
              <Route path="export" element={<ExportData />} />
              <Route path="adjn-summary" element={<AdjudicationSummaryReport />} />
              <Route path="monthly-report" element={<MonthlyReportPage />} />
              <Route path="br-dr" element={<BRDRLookupPage />} />
            </Routes>
          </QueryErrorBoundary>
        </div>
      </main>
    </div>
  );
}
