import { NavLink, Outlet, useNavigate, useLocation, Navigate } from 'react-router-dom';
import { useState } from 'react';
import { ShieldAlert, FileText, Plus, LogOut, Menu, User, KeyRound, Users, ClipboardList } from 'lucide-react';
import { useAuth } from '@/contexts/AuthContext';

export default function SDOLayout() {
  const { user, logout } = useAuth();
  const navigate = useNavigate();
  const location = useLocation();
  const [isCollapsed, setIsCollapsed] = useState(true);

  const currentDate = new Date().toLocaleDateString('en-GB', {
    weekday: 'short', day: '2-digit', month: 'short', year: 'numeric'
  });

  const handleLogout = () => {
    logout();
    navigate('/login');
  };

  return (
    <div className="flex min-h-screen w-full print:bg-white print:overflow-visible" style={{ background: '#f0f4f8' }}>
      {/* Sidebar */}
      <aside
        className={`${isCollapsed ? 'w-16' : 'w-64'} flex flex-col shadow-xl transition-[width] duration-300 ease-in-out z-50 shrink-0 print:!hidden sticky top-0 h-screen overflow-hidden`}
        style={{ background: 'linear-gradient(180deg, #1e3a5f 0%, #0f2340 100%)' }}
      >
        {/* Logo Area & Toggle */}
        <div className="p-3 border-b border-white/10 flex items-center justify-between min-h-[72px] shrink-0">
            <div
              className="flex items-center gap-3 transition-[opacity,transform] duration-300 ease-in-out"
              style={{ opacity: isCollapsed ? 0 : 1, transform: isCollapsed ? 'translateX(-8px)' : 'translateX(0)', width: isCollapsed ? 0 : 'auto', overflow: 'hidden', pointerEvents: isCollapsed ? 'none' : 'auto' }}
            >
                <div className="bg-blue-500/20 border border-blue-400/30 p-2 rounded-lg shrink-0">
                  <ShieldAlert className="w-6 h-6 text-blue-300" />
                </div>
                <div className="flex-1 min-w-0 whitespace-nowrap">
                  <h1 className="text-white font-bold text-base tracking-wide leading-tight">SDO MODULE</h1>
                  <p className="text-blue-300 text-xs">COPS</p>
                </div>
            </div>
            
            <button
              type="button"
              onClick={() => setIsCollapsed(v => !v)}
              className={`text-slate-300 hover:text-white hover:bg-white/10 p-2 rounded-lg transition-colors ${isCollapsed ? 'mx-auto' : 'ml-1 shrink-0'}`}
              title={isCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
            >
              <Menu size={26} />
            </button>
          </div>

        {/* Navigation */}
        <div className="flex-1 overflow-y-auto overflow-x-hidden overscroll-contain min-h-0" style={{ opacity: 1, transition: 'opacity 200ms ease' }}>
          <nav className="p-3 space-y-1 mt-2">
            
            {user?.user_status !== 'TEMP' && (
              <>
                <p className="text-blue-400/70 text-xs uppercase tracking-widest font-semibold px-3 mb-3 whitespace-nowrap transition-opacity duration-300" style={{ opacity: isCollapsed ? 0 : 1, height: isCollapsed ? 0 : 'auto', overflow: 'hidden', margin: isCollapsed ? 0 : undefined }}>Offence Cases</p>
                <NavItem to="/sdo/offence/new" icon={<Plus size={24}/>} label="Register New O/S Case" color="blue" id="nav-new-os" end={false} collapsed={isCollapsed} />
                <NavItem to="/sdo/offline-adjudication" icon={<ClipboardList size={24}/>} label="Offline Adjudication" color="blue" id="nav-offline-adj" end={false} collapsed={isCollapsed} />
                <NavItem to="/sdo/offence" icon={<FileText size={24}/>} label="View All O/S Cases" color="blue" id="nav-list-os" end collapsed={isCollapsed} />
              </>
            )}

            {user?.user_status !== 'TEMP' && (
              <>
                <p className="text-blue-400/70 text-xs uppercase tracking-widest font-semibold px-3 mt-4 mb-3 whitespace-nowrap transition-opacity duration-300" style={{ opacity: isCollapsed ? 0 : 1, height: isCollapsed ? 0 : 'auto', overflow: 'hidden', margin: isCollapsed ? 0 : undefined }}>Administration</p>
                <NavItem to="/sdo/users" icon={<Users size={24}/>} label="Manage Users" color="blue" id="nav-manage-users" end={false} collapsed={isCollapsed} />
              </>
            )}
            
          </nav>

          <div className="mt-6 px-3">
            <p className="text-blue-400/70 text-xs uppercase tracking-widest font-semibold px-3 mb-3 whitespace-nowrap transition-opacity duration-300" style={{ opacity: isCollapsed ? 0 : 1, height: isCollapsed ? 0 : 'auto', overflow: 'hidden', margin: isCollapsed ? 0 : undefined }}>Account</p>
            <NavItem to="/sdo/change-password" icon={<KeyRound size={24}/>} label="Change Password" color="blue" id="nav-change-pwd" end={false} collapsed={isCollapsed} />
          </div>
        </div>

        {/* Footer */}
        <div className={`border-t border-white/10 space-y-3 shrink-0 ${isCollapsed ? 'p-2' : 'p-4'}`}>
          {/* User Info */}
          <div
            className="bg-white/5 border border-white/10 rounded-lg transition-[opacity,max-height] duration-300 ease-in-out overflow-hidden"
            style={{ opacity: isCollapsed ? 0 : 1, maxHeight: isCollapsed ? 0 : '120px', padding: isCollapsed ? 0 : '0.75rem', borderColor: isCollapsed ? 'transparent' : undefined }}
          >
              <div className="flex items-center gap-2 mb-1 whitespace-nowrap">
                <User size={18} className="text-blue-300" />
                <p className="text-blue-200 text-xs uppercase tracking-wider font-semibold">Active Officer</p>
              </div>
              <p className="text-white font-semibold text-sm truncate">{user?.user_name}</p>
              <p className="text-blue-300 text-xs whitespace-nowrap">{user?.user_desig || user?.user_role} · {currentDate}</p>
          </div>


          
          <button
            onClick={handleLogout}
            id="btn-sdo-logout"
            className={`flex w-full items-center rounded-lg text-blue-200 hover:bg-red-500/20 hover:text-red-300 transition-colors text-base focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-400 focus-visible:ring-offset-2 focus-visible:ring-offset-[#0f2340] ${isCollapsed ? 'justify-center py-3' : 'gap-2 px-4 py-3'}`}
            title="Sign Out"
          >
            <LogOut size={22} />
            {!isCollapsed && 'Sign Out'}
          </button>
        </div>
      </aside>

      {/* Main Content */}
      <main className="flex-1 flex flex-col min-h-screen print:h-auto print:overflow-visible">
        <div className="flex-1 min-h-0 p-4 md:p-6 print:overflow-visible print:p-0">
          {user?.user_status === 'TEMP' && !location.pathname.endsWith('/users') ? (
            <Navigate to="/sdo/users" replace />
          ) : (
            <Outlet />
          )}
        </div>
      </main>
    </div>
  );
}

function NavItem({ to, icon, label, color, id, end, collapsed }: {
  to: string, icon: React.ReactNode, label: string, color: string, id?: string, end?: boolean, collapsed?: boolean
}) {
  const activeColor = color === 'blue' ? 'bg-blue-500/30 border-blue-400/40 text-white' : 'bg-amber-500/30 border-amber-400/40 text-white';
  const hoverColor = color === 'blue' ? 'hover:bg-blue-500/15 hover:text-white' : 'hover:bg-amber-500/15 hover:text-white';

  return (
    <NavLink
      to={to}
      end={end}
      id={id}
      className={({ isActive }) =>
        `flex items-center ${collapsed ? 'justify-center py-3' : 'gap-3 px-4 py-3'} rounded-lg transition-colors text-base border ${isActive ? `${activeColor} border` : `text-blue-200 border-transparent ${hoverColor}`}`
      }
      title={collapsed ? label : undefined}
    >
      <span className="opacity-90">{icon}</span>
      {!collapsed && <span className="font-medium leading-tight">{label}</span>}
    </NavLink>
  );
}
