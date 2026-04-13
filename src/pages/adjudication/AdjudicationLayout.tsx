import { NavLink, Outlet, useNavigate, useLocation, Navigate } from 'react-router-dom';
import { Gavel, FileText, CheckCircle, LogOut, Menu, User, KeyRound, Users, ClipboardCheck } from 'lucide-react';
import { useAuth } from '@/contexts/AuthContext';
import { useState, useEffect, useMemo } from 'react';

// Static style objects — defined once, never reallocated on render
const SIDEBAR_STYLE = { background: 'linear-gradient(180deg, #78350f 0%, #451a03 100%)' } as const;
const MAIN_STYLE = { background: '#fefce8' } as const;
import api from '@/lib/api';

export default function AdjudicationLayout() {
  const { user, token, logout } = useAuth();
  const navigate = useNavigate();
  const [pendingCount, setPendingCount] = useState<number | null>(null);
  const [offlinePendingCount, setOfflinePendingCount] = useState<number | null>(null);
  const [isCollapsed, setIsCollapsed] = useState(true);
  const location = useLocation();

  // useMemo — toLocaleDateString with Intl options is not free; layout re-renders on every route change
  const currentDate = useMemo(() => new Date().toLocaleDateString('en-GB', {
    weekday: 'short', day: '2-digit', month: 'short', year: 'numeric'
  }), []);

  // Refresh badge counts on every route change so the numbers stay accurate
  // after adjudication, rejection, or deletion without requiring a full page reload.
  useEffect(() => {
    if (!token) return;
    api.get('/os/sidebar-counts', { headers: { Authorization: `Bearer ${token}` } })
      .then(res => {
        setPendingCount(res.data.pending);
        setOfflinePendingCount(res.data.offline_pending);
      })
      .catch(() => {
        setPendingCount(null);
        setOfflinePendingCount(null);
      });
  }, [token, location.pathname]);

  const handleLogout = () => {
    logout();
    navigate('/login');
  };

  return (
    <div className="flex min-h-screen w-full print:bg-white print:overflow-visible" style={MAIN_STYLE}>
      {/* Sidebar */}
      <aside
        className={`${isCollapsed ? 'w-16' : 'w-64'} flex flex-col shadow-xl transition-[width] duration-300 ease-in-out z-50 shrink-0 print:!hidden sticky top-0 h-screen overflow-hidden`}
        style={SIDEBAR_STYLE}
      >
        {/* Logo */}
        <div className="p-3 border-b border-white/10 flex items-center justify-between min-h-[72px] shrink-0">
            <div
              className="flex items-center gap-3 transition-[opacity,transform] duration-300 ease-in-out"
              style={{ opacity: isCollapsed ? 0 : 1, transform: isCollapsed ? 'translateX(-8px)' : 'translateX(0)', width: isCollapsed ? 0 : 'auto', overflow: 'hidden', pointerEvents: isCollapsed ? 'none' : 'auto' }}
            >
                <div className="bg-amber-500/20 border border-amber-400/30 p-2 rounded-lg shrink-0">
                  <Gavel className="w-6 h-6 text-amber-300" />
                </div>
                <div className="flex-1 min-w-0 whitespace-nowrap">
                  <h1 className="text-white font-bold text-base tracking-wide leading-tight">ONLINE ADJN</h1>
                  <p className="text-amber-300 text-xs">Adjudication Module</p>
                </div>
            </div>
            
            <button
              type="button"
              onClick={() => setIsCollapsed(v => !v)}
              className={`text-amber-200 hover:text-white hover:bg-white/10 p-2 rounded-lg transition-colors ${isCollapsed ? 'mx-auto' : 'ml-1 shrink-0'}`}
              title={isCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
            >
              <Menu size={26} />
            </button>
          </div>

        {/* Navigation */}
        <div className="flex-1 overflow-y-auto overflow-x-hidden overscroll-contain min-h-0">
          <nav className="p-3 space-y-1 mt-2">
            
            {user?.user_status !== 'TEMP' && (
              <>
                {!isCollapsed && <p className="text-amber-400/70 text-xs uppercase tracking-widest font-semibold px-3 mb-3 whitespace-nowrap transition-opacity duration-300" style={{ opacity: isCollapsed ? 0 : 1 }}>O/S Cases</p>}
                <AdjNavItem
                  to="/adjudication/pending"
                  icon={<FileText size={24} />}
                  label="Pending Adjudication"
                  id="nav-pending"
                  badge={pendingCount}
                  collapsed={isCollapsed}
                />
                <AdjNavItem
                  to="/adjudication/adjudicated"
                  icon={<CheckCircle size={24} />}
                  label="Adjudicated Cases"
                  id="nav-adjudicated"
                  collapsed={isCollapsed}
                />
                <AdjNavItem
                  to="/adjudication/offline-pending"
                  icon={<ClipboardCheck size={24} />}
                  label="Offline Adjudication"
                  id="nav-offline-pending"
                  badge={offlinePendingCount}
                  collapsed={isCollapsed}
                />
              </>
            )}

            {user?.user_status !== 'TEMP' && (
              <>
                {!isCollapsed && <p className="text-amber-400/70 text-xs uppercase tracking-widest font-semibold px-3 mt-4 mb-3 whitespace-nowrap transition-opacity duration-300" style={{ opacity: isCollapsed ? 0 : 1 }}>Administration</p>}
                <AdjNavItem
                  to="/adjudication/users"
                  icon={<Users size={24} />}
                  label="Manage Users"
                  id="nav-manage-users"
                  collapsed={isCollapsed}
                />
              </>
            )}
            
          </nav>

          <div className="mt-6 px-3">
            {!isCollapsed && <p className="text-amber-400/70 text-xs uppercase tracking-widest font-semibold px-3 mb-3 whitespace-nowrap transition-opacity duration-300" style={{ opacity: isCollapsed ? 0 : 1 }}>Account</p>}
            <AdjNavItem to="/adjudication/change-password" icon={<KeyRound size={24} />} label="Change Password" id="nav-adjn-pwd" collapsed={isCollapsed} />
          </div>
        </div>

        {/* Footer */}
        <div className={`border-t border-white/10 space-y-3 shrink-0 ${isCollapsed ? 'p-2' : 'p-4'}`}>
          <div
            className="bg-white/5 border border-white/10 rounded-lg transition-[opacity,max-height] duration-300 ease-in-out overflow-hidden"
            style={{ opacity: isCollapsed ? 0 : 1, maxHeight: isCollapsed ? 0 : '120px', padding: isCollapsed ? 0 : '0.75rem', borderColor: isCollapsed ? 'transparent' : undefined }}
          >
              <div className="flex items-center gap-2 mb-1 whitespace-nowrap">
                <User size={18} className="text-amber-300" />
                <p className="text-amber-200 text-xs uppercase tracking-wider font-semibold">Adjudicating Officer</p>
              </div>
              <p className="text-white font-semibold text-sm truncate">{user?.user_name}</p>
              <p className="text-amber-300 text-xs whitespace-nowrap">{user?.user_desig || user?.user_role} · {currentDate}</p>
          </div>


          
          <button
            onClick={handleLogout}
            id="btn-adjn-logout"
            className={`flex w-full items-center rounded-lg text-amber-200 hover:bg-red-500/20 hover:text-red-300 transition-colors text-base focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber-400 focus-visible:ring-offset-2 focus-visible:ring-offset-[#451a03] ${isCollapsed ? 'justify-center py-3' : 'gap-2 px-4 py-3'}`}
            title="Sign Out"
          >
            <LogOut size={22} />
            {!isCollapsed && 'Sign Out'}
          </button>
        </div>
      </aside>

      {/* Main Content */}
      <main className="flex-1 flex flex-col min-h-screen print:h-auto print:overflow-visible">
        <div className="flex-1 min-h-0 p-5 md:p-7 bg-amber-50 print:overflow-visible print:p-0">
          {user?.user_status === 'TEMP' && !location.pathname.endsWith('/users') ? (
            <Navigate to="/adjudication/users" replace />
          ) : (
            <Outlet />
          )}
        </div>
      </main>
    </div>
  );
}

function AdjNavItem({ to, icon, label, id, badge, end, collapsed }: {
  to: string, icon: React.ReactNode, label: string, id?: string, badge?: number | null, end?: boolean, collapsed?: boolean
}) {
  return (
    <NavLink
      to={to}
      end={end}
      id={id}
      title={collapsed ? label : undefined}
      className={({ isActive }) =>
        `flex items-center ${collapsed ? 'justify-center relative py-3' : 'gap-3 px-4 py-3'} rounded-lg transition-colors text-base border ${
          isActive
            ? 'bg-amber-500/30 border-amber-400/40 text-white'
            : 'text-amber-200 border-transparent hover:bg-amber-500/15 hover:text-white'
        }`
      }
    >
      <span className="opacity-90 shrink-0">{icon}</span>
      {!collapsed && <span className="font-medium flex-1 leading-tight">{label}</span>}
      {!collapsed && badge != null && badge > 0 && (
        <span className="bg-red-500 text-white text-xs font-bold px-2 py-0.5 rounded-full min-w-[20px] text-center">
          {badge}
        </span>
      )}
      {collapsed && badge != null && badge > 0 && (
        <span className="absolute top-1 right-1 bg-red-500 text-white text-[10px] font-bold w-4 h-4 rounded-full flex items-center justify-center">
          {badge > 9 ? '9+' : badge}
        </span>
      )}
    </NavLink>
  );
}
