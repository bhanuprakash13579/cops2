import { createContext, useContext, useState, useEffect, useMemo, ReactNode } from 'react';
import api from '@/lib/api';

interface User {
  id?: number;
  user_id: string;
  user_name: string;
  user_desig?: string;
  user_role: string;
  user_status: string;
  /** The designated user admin — the one AC/DC who may add, close, and reset
   *  accounts. Set from the login response and /me; the backend is the authority. */
  is_user_admin?: boolean;
}

interface AuthContextType {
  user: User | null;
  token: string | null;
  login: (token: string, userData: User) => void;
  logout: () => void;
  isAuthenticated: boolean;
  canAccessSDO: () => boolean;
  canAccessAdjudication: () => boolean;
  canAccessQuery: () => boolean;
  canAccessApis: () => boolean;
}

const SDO_ROLES = ['SDO'];
const ADJN_ROLES = ['DC', 'AC'];

const AuthContext = createContext<AuthContextType | undefined>(undefined);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [token, setToken] = useState<string | null>(() => localStorage.getItem('cops_token'));
  const [user, setUser] = useState<User | null>(() => {
    const storedUser = localStorage.getItem('cops_user');
    if (!storedUser) return null;
    try {
      return JSON.parse(storedUser);
    } catch {
      localStorage.removeItem('cops_user');
      return null;
    }
  });

  const login = (newToken: string, userData: User) => {
    localStorage.setItem('cops_token', newToken);
    localStorage.setItem('cops_user', JSON.stringify(userData));
    setToken(newToken);
    setUser(userData);
  };

  const logout = () => {
    localStorage.removeItem('cops_token');
    localStorage.removeItem('cops_user');
    setToken(null);
    setUser(null);
  };

  useEffect(() => {
    const handleAuthDeclined = () => { logout(); };
    window.addEventListener('auth_declined', handleAuthDeclined);
    return () => window.removeEventListener('auth_declined', handleAuthDeclined);
  }, []);

  // Reconcile the cached profile with the server whenever a session starts.
  // The stored copy can lag reality: the user admin may have been designated or
  // stood down, or this account's password reset (status → TEMP) — the menu, the
  // route guards, and the forced password change all read these, so a stale copy
  // would show the wrong thing until the next sign-in. A network error leaves the
  // cached copy alone; only a 401 (via the api interceptor) ends the session.
  useEffect(() => {
    if (!token) return;
    let cancelled = false;
    api.get('/auth/me').then(res => {
      if (cancelled || !res?.data) return;
      setUser(prev => {
        const merged = { ...(prev || {}), ...res.data } as User;
        localStorage.setItem('cops_user', JSON.stringify(merged));
        return merged;
      });
    }).catch(() => { /* offline or 401; interceptor handles 401 */ });
    return () => { cancelled = true; };
  }, [token]);

  const canAccessSDO = () => !!user && SDO_ROLES.includes(user.user_role);
  const canAccessAdjudication = () => !!user && ADJN_ROLES.includes(user.user_role);
  const canAccessQuery = () => !!user && (SDO_ROLES.includes(user.user_role) || ADJN_ROLES.includes(user.user_role));
  const canAccessApis = () => !!user && (SDO_ROLES.includes(user.user_role) || ADJN_ROLES.includes(user.user_role));

  const value = useMemo(() => ({
    user, token, login, logout,
    isAuthenticated: !!token,
    canAccessSDO, canAccessAdjudication, canAccessQuery, canAccessApis,
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }), [user, token]);

  return (
    <AuthContext.Provider value={value}>
      {children}
    </AuthContext.Provider>
  );
}

// eslint-disable-next-line react-refresh/only-export-components
export function useAuth() {
  const context = useContext(AuthContext);
  if (context === undefined) {
    throw new Error('useAuth must be used within an AuthProvider');
  }
  return context;
}
