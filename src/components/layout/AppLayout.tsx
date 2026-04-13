import React, { useEffect } from 'react';
import { NavLink, Outlet, useNavigate, useLocation } from 'react-router-dom';
import { Plane, Gavel, FileText, Database, ShieldAlert, LogOut, Search, Settings } from 'lucide-react';
import { clsx } from "clsx";
import { twMerge } from "tailwind-merge";
import { useAuth } from '@/contexts/AuthContext';

function cn(...inputs: (string | undefined | null | false)[]) {
  return twMerge(clsx(inputs));
}

const TITLE_MAP: [string, string][] = [
  ['/offence/new',    'New O.S. — COPS'],
  ['/offence/',       'Edit O.S. — COPS'],
  ['/offence',        'Offence Cases — COPS'],
  ['/baggage/new',    'New Baggage Receipt — COPS'],
  ['/baggage/',       'Edit Baggage Receipt — COPS'],
  ['/baggage',        'Baggage Receipts — COPS'],
  ['/detention/new',  'New Detention — COPS'],
  ['/detention/',     'Edit Detention — COPS'],
  ['/detention',      'Detention (DR) — COPS'],
  ['/query',          'Search / Query — COPS'],
  ['/reports',        'Reports — COPS'],
  ['/master',         'System Settings — COPS'],
  ['/dashboard',      'Dashboard — COPS'],
  ['/adjudication',   'Adjudication — COPS'],
];

export default function AppLayout() {
  const { user, logout } = useAuth();
  const navigate = useNavigate();
  const location = useLocation();

  useEffect(() => {
    const match = TITLE_MAP.find(([prefix]) => location.pathname.startsWith(prefix));
    document.title = match ? match[1] : 'COPS';
  }, [location.pathname]);
  // TODO: Fetch current shift details from an API Context
  const currentBatch = "10 AM TO 10 PM";
  const _d = new Date();
  const currentDate = `${String(_d.getDate()).padStart(2,'0')}/${String(_d.getMonth()+1).padStart(2,'0')}/${_d.getFullYear()}`;

  const handleLogout = () => {
    logout();
    navigate('/login');
  };

  return (
    <div className="flex min-h-screen w-full bg-slate-50 text-slate-800 print:bg-white print:overflow-visible">
      
      {/* Sidebar Navigation */}
      <aside className="w-64 glass-header flex flex-col justify-between hidden md:flex print:!hidden sticky top-0 h-screen overflow-y-auto">
        <div>
          <div className="p-5 flex items-center space-x-3 border-b border-white/20">
            <div className="bg-white/20 p-2 rounded-lg">
              <ShieldAlert className="w-6 h-6 text-accent-400" />
            </div>
            <div>
              <h1 className="text-lg font-bold tracking-wider">COPS</h1>
              <p className="text-xs text-brand-200">Customs Chennai</p>
            </div>
          </div>
          
          <nav className="p-4 space-y-1">
            <NavItem to="/dashboard" icon={<Database size={20}/>} label="Dashboard" />
            <NavItem to="/baggage" icon={<Plane size={20}/>} label="Baggage Receipts" />
            <NavItem to="/offence" icon={<Gavel size={20}/>} label="Offence Cases" />
            <NavItem to="/detention" icon={<FileText size={20}/>} label="Detention (DR)" />
            <NavItem to="/query" icon={<Search size={20}/>} label="Master Search / Query" />
            <NavItem to="/reports" icon={<FileText size={20}/>} label="Reports & Stats" />
            <NavItem to="/master" icon={<Settings size={20}/>} label="System Settings" />
          </nav>
        </div>

        <div className="p-4 border-t border-white/20 space-y-2 text-sm">
          <div className="bg-brand-800/50 p-3 rounded-md border border-white/10">
            <p className="text-brand-200 text-xs uppercase tracking-wider mb-1">Active Shift</p>
            <p className="font-medium text-white">{currentDate}</p>
            <p className="font-semibold text-accent-400">{currentBatch}</p>
          </div>
          <div className="bg-brand-800/50 p-3 rounded-md border border-white/10 mt-2 mb-2">
             <p className="text-brand-200 text-xs uppercase tracking-wider mb-1">Active User</p>
             <p className="font-medium text-white capitalize">{user?.user_name || 'Admin'}</p>
             <p className="text-xs text-slate-300">{user?.user_role || 'System Officer'}</p>
          </div>
          <button className="flex w-full items-center p-2 rounded-md hover:bg-white/10 text-brand-100 transition-colors mt-2">
            <Settings size={18} className="mr-3" />
            Settings
          </button>
          <button 
            onClick={handleLogout}
            className="flex w-full items-center p-2 rounded-md hover:bg-white/10 text-brand-100 transition-colors"
          >
            <LogOut size={18} className="mr-3" />
            Sign Out
          </button>
        </div>
      </aside>

      {/* Main Content Area */}
      <main className="flex-1 flex flex-col min-h-screen print:h-auto print:overflow-visible">
        <div className="flex-1 p-4 md:p-8 print:p-0 print:overflow-visible">
          <Outlet />
        </div>
      </main>
    </div>
  );
}

function NavItem({ to, icon, label }: { to: string, icon: React.ReactNode, label: string }) {
  return (
    <NavLink 
      to={to} 
      className={({isActive}) => cn(
        "flex items-center px-4 py-3 rounded-md transition-colors duration-200 mb-1",
        isActive 
          ? "bg-brand-500/30 text-white shadow-inner border border-white/10" 
          : "text-brand-100 hover:bg-white/10 hover:text-white"
      )}
    >
      <span className="mr-3">{icon}</span>
      <span className="font-medium">{label}</span>
    </NavLink>
  )
}
