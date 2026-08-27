/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia 
 * @license        MIT (https://www.tokensbyte.ai/)
 */

import React, { useEffect, useState } from 'react';
import { BrowserRouter as Router, Routes, Route, Navigate, useLocation, useParams } from 'react-router-dom';
import { Spin } from 'antd';
import { useTranslation } from 'react-i18next';
import axios from 'axios';
import { captureInviteFromSearch } from './utils/inviteTracking';
import { coalesceAsync } from './utils/coalesceAsync';
import { fetchActivePlugins } from './utils/activePlugins';
import Login from './pages/Login/Login';
import Register from './pages/Login/Register';
import ForgotPassword from './pages/Login/ForgotPassword';
import AdminLogin from './pages/AdminLogin/AdminLogin';
import LegalPage from './pages/Legal/LegalPage';
import DashboardLayout from './layouts/DashboardLayout';
import Dashboard from './pages/Dashboard/Dashboard';
import Channels from './pages/Channels/Channels';
import ModelChannelsDisplay from './pages/Channels/ModelChannelsDisplay';
import ChannelTest from './pages/Channels/ChannelTest';
import ChannelConfigs from './pages/Channels/ChannelConfigs';
import UpstreamAssetBindings from './pages/Channels/UpstreamAssetBindings';
import Models from './pages/Models/Models';
import ForwardRules from './pages/Models/ForwardRules';
import BillingRules from './pages/Models/BillingRules';
import Tokens from './pages/Tokens/Tokens';
import Upstreams from './pages/Upstreams/Upstreams';

import Users from './pages/Users/Users';
import UserLevels from './pages/Users/UserLevels';
import UserLevelEdit from './pages/Users/UserLevelEdit';
import AdminGroups from './pages/Users/AdminGroups';
import AdminGroupEdit from './pages/Users/AdminGroupEdit';
import Logs from './pages/Logs/Logs';
import TaskLogs from './pages/Logs/TaskLogs';
import {
  RelayAPI,
  PortalDocsViewer,
  PluginsList,
  PluginConfig,
  ArkUserDashboard,
  UserAssets,
  AdvancedMarketing,
  ThemePromoLanding,
  Playground,
  PlaygroundHome,
  Playground2026,
  PlaygroundHome2026,
  WorkflowCreateBootstrap2026,
  ModelMarketplace,
} from './plugins-registry';
import Redemptions from './pages/Redemptions/Redemptions';
import Profile from './pages/Profile/Profile';
import NotificationSubscription from './pages/Profile/NotificationSubscription';
import Wallet from './pages/Wallet/Wallet';
import RechargeRecords from './pages/Finance/RechargeRecords';
import GiftRecords from './pages/Finance/GiftRecords';
import FinanceDataAnalysis from './pages/Finance/FinanceDataAnalysis';
import OrderDetails from './pages/Finance/OrderDetails';
import Settings from './pages/admin/Settings';
import PaymentSettings from './pages/admin/PaymentSettings';
import MessageNotification from './pages/admin/MessageNotification';
import OAuthSettings from './pages/admin/OAuthSettings';
import RegistrationGifts from './pages/admin/Marketing/RegistrationGifts';
import Announcements from './pages/admin/Marketing/Announcements';
import SystemAbout from './pages/admin/SystemAbout';
import useAuthStore from './store/auth';
import useSettingsStore from './store/settings';
import { clearAwaitingFreshSetup, DEFAULT_ADMIN_PATH, fetchAdminInitStatus, isAwaitingFreshSetup } from './utils/freshSetup';


const PrivateRoute = ({ children, adminOnly = false, userOnly = false }: { children: React.ReactNode, adminOnly?: boolean, userOnly?: boolean }) => {
  const { token, user } = useAuthStore();
  const adminPath = localStorage.getItem('tokensbyte_admin_path') || 'admin1688';
  // 管理端未登录应回管理登录页，而非用户端 /login
  if (!token) return <Navigate to={adminOnly ? `/${adminPath}` : '/login'} replace />;
  if (adminOnly && user?.role !== 'admin') return <Navigate to="/dashboard" replace />;
  if (userOnly && user?.role === 'admin') return <Navigate to={`/${adminPath}/dashboard`} replace />;
  return <>{children}</>;
};

/** 兼容旧画布路径 /playground-2026/:projectId → 工作流列表（项目画布已下线） */
const Playground2026LegacyRedirect = () => {
  return <Navigate to="/playground-2026/workflows" replace />;
};

const Playground2026WorksAlbumRedirect = () => {
  const { albumId } = useParams<{ albumId: string }>();
  if (!albumId) {
    return <Navigate to="/playground-2026/assets/works" replace />;
  }
  return <Navigate to={`/playground-2026/assets/albums/${albumId}`} replace />;
};

/** 后端编译重启 / 代理网关暂时不可达等瞬时网络错误（与 request 拦截器 502/503/504 对齐） */
function isTransientNetworkError(error: any): boolean {
  if (!error || axios.isCancel(error)) return false;
  const status = error?.response?.status;
  if (!error?.response) return true;
  return status === 502 || status === 503 || status === 504;
}

const sleep = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));

/**
 * 全新安装（尚无管理员）时，任意入口都直接进入管理员初始化页。
 * 清空数据库后会带 awaiting_setup 标记：确认无管理员后进入初始化页。
 */
const AdminSetupGate = ({ children }: { children: React.ReactNode }) => {
  const [bootState, setBootState] = useState<'loading' | 'ready' | 'need_setup'>('loading');
  const [waitHint, setWaitHint] = useState(false);
  const awaitingSetup = isAwaitingFreshSetup();
  const adminPath = awaitingSetup
    ? DEFAULT_ADMIN_PATH
    : (localStorage.getItem('tokensbyte_admin_path') || DEFAULT_ADMIN_PATH);

  useEffect(() => {
    let cancelled = false;
    const enterNeedSetup = () => {
      const { token, setToken, setUser } = useAuthStore.getState();
      if (token) {
        setToken(null);
        setUser(null);
      }
      localStorage.setItem('tokensbyte_admin_path', DEFAULT_ADMIN_PATH);
      clearAwaitingFreshSetup();
      setBootState('need_setup');
    };

    const check = async () => {
      const awaiting = isAwaitingFreshSetup();
      while (!cancelled) {
        try {
          const initialized = awaiting
            ? await fetchAdminInitStatus()
            : await coalesceAsync(
                'auth:admin-init-status',
                fetchAdminInitStatus,
                { recentMs: 10_000 },
              );
          if (cancelled) return;
          if (!initialized) {
            enterNeedSetup();
            return;
          }
          clearAwaitingFreshSetup();
          setBootState('ready');
          return;
        } catch {
          if (cancelled) return;
          if (awaiting) {
            await sleep(1000);
            continue;
          }
          // 后端短暂不可用时不锁死在初始化页（清空后等待除外）
          setBootState('ready');
          return;
        }
      }
    };
    void check();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!awaitingSetup) return;
    const timer = window.setTimeout(() => setWaitHint(true), 8000);
    return () => window.clearTimeout(timer);
  }, [awaitingSetup]);

  if (bootState === 'loading') {
    return (
      <div style={{ display: 'flex', height: '100vh', alignItems: 'center', justifyContent: 'center', color: '#666' }}>
        {awaitingSetup ? (
          <div style={{ textAlign: 'center', lineHeight: 1.7 }}>
            <Spin style={{ marginBottom: 16 }} />
            <div>服务准备中，即将进入全新安装…</div>
            <div style={{ opacity: 0.75 }}>Preparing service, entering fresh setup…</div>
            {waitHint && (
              <div style={{ marginTop: 16, fontSize: 13, opacity: 0.8 }}>
                <div>若长时间停留，请确认后端已启动后刷新本页</div>
                <div style={{ opacity: 0.75 }}>If this stays, start the backend and refresh</div>
              </div>
            )}
          </div>
        ) : 'Loading...'}
      </div>
    );
  }

  if (bootState === 'need_setup') {
    return (
      <Routes>
        <Route path={`/${adminPath}`} element={<AdminLogin />} />
        <Route path="*" element={<Navigate to={`/${adminPath}`} replace />} />
      </Routes>
    );
  }

  return <>{children}</>;
};

const PluginRoute = ({
  children,
  pluginName,
  allowGuest = false,
}: {
  children: React.ReactNode;
  pluginName: string;
  allowGuest?: boolean;
}) => {
  const isPublicPlugin =
    allowGuest &&
    ['site_portal_pro', 'site_portal', 'docs_api', 'model_marketplace'].includes(pluginName);
  // loading | waiting_network | ready | denied
  const [phase, setPhase] = React.useState<
    'loading' | 'waiting_network' | 'ready' | 'denied'
  >(isPublicPlugin ? 'ready' : 'loading');
  const [isActive, setIsActive] = React.useState(isPublicPlugin);
  const { user } = useAuthStore();

  React.useEffect(() => {
    if (isPublicPlugin) return undefined;

    let cancelled = false;
    let timer: ReturnType<typeof setInterval> | null = null;

    const resolveAccess = (matched: any): boolean => {
      if (!matched) return false;
      if (
        allowGuest &&
        (matched.mp_allow_guest ||
          ['site_portal_pro', 'site_portal', 'docs_api', 'model_marketplace'].includes(pluginName))
      ) {
        return true;
      }
      if (!user) return false;
      if (user?.role === 'admin' || matched.allowed_levels === 'all') return true;
      const userGroup = user?.user_group || '';
      const levelId = user?.level_id != null ? String(user.level_id) : '';
      const levels = String(matched.allowed_levels || '').split(',');
      return levels.includes(userGroup) || (levelId !== '' && levels.includes(levelId));
    };

    const checkPlugin = async () => {
      try {
        // 对齐 Dashboard /metrics/live：瞬时失败静默重试，不打断页面
        // 与 Layout / 各页共用 coalesce，避免刷新重复打 /plugins/active
        const response: any = await fetchActivePlugins();
        if (cancelled) return;
        const plugins: any[] = response?.active_plugins || [];
        const matched = plugins.find((p: any) => p.name === pluginName);
        const ok = resolveAccess(matched);
        setIsActive(ok);
        setPhase(ok ? 'ready' : 'denied');
        if (timer) {
          clearInterval(timer);
          timer = null;
        }
      } catch (e) {
        if (cancelled) return;
        if (isTransientNetworkError(e)) {
          // 网络/编译重启：留在当前路由等待，周期性重试（同 metrics/live 5s）
          setPhase((prev) => (prev === 'ready' ? prev : 'waiting_network'));
          if (!timer) {
            timer = setInterval(() => {
              void checkPlugin();
            }, 5000);
          }
          return;
        }
        // 非瞬时错误仍视为不可用
        setIsActive(false);
        setPhase('denied');
        if (timer) {
          clearInterval(timer);
          timer = null;
        }
      }
    };

    void checkPlugin();
    return () => {
      cancelled = true;
      if (timer) clearInterval(timer);
    };
  }, [pluginName, user, allowGuest, isPublicPlugin]);

  if (phase === 'loading' || phase === 'waiting_network') {
    return (
      <div
        style={{
          minHeight: '100vh',
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          gap: 12,
          padding: 24,
        }}
      >
        <Spin size="large" />
        <div style={{ fontSize: 13, opacity: 0.65, textAlign: 'center' }}>
          {phase === 'waiting_network'
            ? '后端服务连接中，请稍候…'
            : '加载中…'}
        </div>
      </div>
    );
  }

  if (phase === 'denied' || !isActive) {
    if (!user) {
      return <Navigate to="/login" replace />;
    }
    return <Navigate to="/dashboard" replace />;
  }

  return <>{children}</>;
};

/**
 * UserEndRoute – 用户端根路由守卫
 * 当站点门户（或增强版）已启用时，访问精确的 '/' 路径会跳转到后端渲染的门户首页。
 * 增强版优先：site_portal_pro → /home-pro；否则 site_portal → /home。
 * 其他子路径（如 /tokens, /docs 等）仍需登录后才能访问。
 */
const UserEndRoute = () => {
  const { token, user } = useAuthStore();
  const location = useLocation();
  const isRootPath = location.pathname === '/';
  const [checking, setChecking] = useState(isRootPath);
  const [portalHomePath, setPortalHomePath] = useState<string | null>(null);

  useEffect(() => {
    if (!isRootPath) { setChecking(false); return; }
    let cancelled = false;
    fetchActivePlugins()
      .then((res: any) => {
        if (cancelled) return;
        const active: any[] = res?.active_plugins || [];
        if (active.some((p: any) => p.name === 'site_portal_pro')) {
          setPortalHomePath('/home-pro');
        } else if (active.some((p: any) => p.name === 'site_portal')) {
          setPortalHomePath('/home');
        } else {
          setPortalHomePath(null);
        }
      })
      .catch(() => {})
      .finally(() => { if (!cancelled) setChecking(false); });
    return () => { cancelled = true; };
  }, [isRootPath]);

  // 精确 '/' 路径 & 门户已启用 → 跳转到后端渲染的门户首页
  if (isRootPath && !checking && portalHomePath) {
    window.location.href = portalHomePath;
    return null;
  }
  // 精确 '/' 路径，仍在检测中 → 展示空白避免闪烁
  if (isRootPath && checking) return null;
  const adminPath = localStorage.getItem('tokensbyte_admin_path') || 'admin1688';
  // 非根路径或门户未启用 → 走常规鉴权逻辑
  if (!token) return <Navigate to="/login" />;
  if (user?.role === 'admin') return <Navigate to={`/${adminPath}/dashboard`} />;
  return <DashboardLayout isUserEnd={true} />;
};

/** SPA 路由变化时再次捕获 aff/team（覆盖 /promo、/register、站内跳转） */
const InviteParamCapture: React.FC = () => {
  const location = useLocation();
  useEffect(() => {
    captureInviteFromSearch(location.search);
  }, [location.search]);
  return null;
};

const App: React.FC = () => {
  const { fetchSettings } = useSettingsStore();
  const adminPath = localStorage.getItem('tokensbyte_admin_path') || 'admin1688';
  const { i18n } = useTranslation();

  useEffect(() => {
    // Attempt to map zh to zh-CN for better semantics, otherwise use the exact i18n language
    document.documentElement.lang = i18n.language === 'zh' ? 'zh-CN' : (i18n.language || 'en');
  }, [i18n.language]);

  // 首屏同步捕获 URL 邀请参数，保证 Register/Login 首渲可读到存储
  React.useMemo(() => {
    captureInviteFromSearch();
  }, []);

  useEffect(() => {
    fetchSettings();
  }, [fetchSettings]);

  return (
    <Router future={{ v7_startTransition: true, v7_relativeSplatPath: true }}>
      <InviteParamCapture />
      <React.Suspense fallback={<div style={{ display: 'flex', justifyContent: 'center', alignItems: 'center', height: '100vh' }}><Spin size="large" /></div>}>
        <AdminSetupGate>
        <Routes>
        {/* Public Routes */}
        <Route path="/login" element={<Login />} />
        <Route path="/register" element={<Register />} />
        <Route path="/forgot-password" element={<ForgotPassword />} />
        <Route path="/legal/:type" element={<LegalPage />} />
        <Route path="/promo/:slug" element={<ThemePromoLanding />} />

        <Route
          path="/playground"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground">
                <PlaygroundHome />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground/:projectId"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground">
                <Playground />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <Navigate to="/playground-2026/assets/works" replace />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/assets/works"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <PlaygroundHome2026 />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/assets/uploads"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <PlaygroundHome2026 />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/assets/favorites"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <PlaygroundHome2026 />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/assets/albums/:albumId"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <PlaygroundHome2026 />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/resources/works"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <Navigate to="/playground-2026/assets/works" replace />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/resources/uploads"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <Navigate to="/playground-2026/assets/uploads" replace />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/resources/favorites"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <Navigate to="/playground-2026/assets/favorites" replace />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/resources/albums/:albumId"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <Playground2026WorksAlbumRedirect />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/works"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <Navigate to="/playground-2026/assets/works" replace />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/works/favorites"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <Navigate to="/playground-2026/assets/favorites" replace />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/works/albums/:albumId"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <Playground2026WorksAlbumRedirect />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/projects"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <Navigate to="/playground-2026/workflows" replace />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/projects/:projectId"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <Navigate to="/playground-2026/workflows" replace />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/images"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <PlaygroundHome2026 />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/images/generate"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <Navigate to="/playground-2026/images" replace />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/videos"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <PlaygroundHome2026 />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/workflows"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <PlaygroundHome2026 />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/workflows/create"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <WorkflowCreateBootstrap2026 />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/workflows/:workflowId"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <Playground2026 />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/videos/generate"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <Navigate to="/playground-2026/videos" replace />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/generate-image"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <Navigate to="/playground-2026/images" replace />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        <Route
          path="/playground-2026/generate-video"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <Navigate to="/playground-2026/videos" replace />
              </PluginRoute>
            </PrivateRoute>
          }
        />
        {/* 兼容旧路径 /playground-2026/:projectId */}
        <Route
          path="/playground-2026/:projectId"
          element={
            <PrivateRoute>
              <PluginRoute pluginName="playground_2026">
                <Playground2026LegacyRedirect />
              </PluginRoute>
            </PrivateRoute>
          }
        />

        {/* Model Marketplace Routes (Full Screen, Independent) */}
        <Route
          path="/home/models"
          element={
            <PluginRoute pluginName="model_marketplace" allowGuest={true}>
              <ModelMarketplace />
            </PluginRoute>
          }
        />
        {/* 保留旧地址兼容已有外链 */}
        <Route
          path="/models"
          element={
            <PluginRoute pluginName="model_marketplace" allowGuest={true}>
              <ModelMarketplace />
            </PluginRoute>
          }
        />

        {/* API 教程：仅绑定 docs_api，与 site_portal_pro 数据源解耦 */}
        <Route
          path="/docs"
          element={
            <PluginRoute pluginName="docs_api" allowGuest={true}>
              <RelayAPI apiPrefix="/plugins/docs-api" />
            </PluginRoute>
          }
        />
        <Route
          path="/docs/:id"
          element={
            <PluginRoute pluginName="docs_api" allowGuest={true}>
              <RelayAPI apiPrefix="/plugins/docs-api" />
            </PluginRoute>
          }
        />
        <Route
          path="/docs/:category/:id"
          element={
            <PluginRoute pluginName="docs_api" allowGuest={true}>
              <RelayAPI apiPrefix="/plugins/docs-api" />
            </PluginRoute>
          }
        />

        {/* 高级门户文档：仅绑定 site_portal_pro，独立路径与库表 */}
        <Route
          path="/home-pro/docs"
          element={
            <PluginRoute pluginName="site_portal_pro" allowGuest={true}>
              <PortalDocsViewer />
            </PluginRoute>
          }
        />
        <Route
          path="/home-pro/docs/:id"
          element={
            <PluginRoute pluginName="site_portal_pro" allowGuest={true}>
              <PortalDocsViewer />
            </PluginRoute>
          }
        />
        <Route
          path="/home-pro/docs/:category/:id"
          element={
            <PluginRoute pluginName="site_portal_pro" allowGuest={true}>
              <PortalDocsViewer />
            </PluginRoute>
          }
        />

        {/* User End Routes (Default) */}
        <Route
          path="/"
          element={<UserEndRoute />}
        >
          {/* index 由 UserEndRoute 处理：门户启用 → /home，否则 → /login */}
          <Route index element={<Dashboard />} />
          <Route path="dashboard" element={<Dashboard />} />
          <Route path="tokens" element={<Tokens />} />
          <Route path="logs" element={<Logs />} />
          <Route path="task-logs" element={<TaskLogs />} />

          <Route path="wallet" element={<Wallet />} />
          <Route path="assets" element={<PluginRoute pluginName="asset_manager"><UserAssets key="asset_manager" pluginNs="asset_manager" /></PluginRoute>} />
          <Route path="assets-intl" element={<PluginRoute pluginName="asset_manager_intl"><UserAssets key="asset_manager_intl" pluginNs="asset_manager_intl" /></PluginRoute>} />
          <Route path="advanced-marketing" element={<PluginRoute pluginName="team_marketing"><AdvancedMarketing /></PluginRoute>} />

          <Route path="ark-video-monitor" element={<PluginRoute pluginName="volcengine_ark_monitor"><ArkUserDashboard /></PluginRoute>} />
          <Route path="profile" element={<Profile />} />
          <Route path="profile/notifications" element={<NotificationSubscription />} />
        </Route>

        {/* System End Routes：index 为管理登录页，子路由需管理员鉴权 */}
        <Route path={`/${adminPath}`}>
          <Route index element={<AdminLogin />} />
          <Route
            element={
              <PrivateRoute adminOnly={true}>
                <DashboardLayout isUserEnd={false} />
              </PrivateRoute>
            }
          >
            <Route path="dashboard" element={<Dashboard />} />
            <Route path="docs" element={<RelayAPI apiPrefix="/plugins/docs-api" />} />
            <Route path="docs/:id" element={<RelayAPI apiPrefix="/plugins/docs-api" />} />
            <Route path="docs/:category/:id" element={<RelayAPI apiPrefix="/plugins/docs-api" />} />
            <Route path="upstreams" element={<Upstreams />} />
            <Route path="channel-configs" element={<ChannelConfigs />} />
            <Route path="upstream-asset-bindings" element={<UpstreamAssetBindings />} />
            <Route path="channels" element={<Channels />} />
            <Route path="channels/model-display" element={<ModelChannelsDisplay />} />
            <Route path="channels/test/:id" element={<ChannelTest />} />
            <Route path="models" element={<Models />} />
            <Route path="forward-rules" element={<ForwardRules />} />
            <Route path="billing-rules" element={<BillingRules />} />
            <Route path="tokens" element={<Tokens />} />

            <Route path="logs" element={<Logs />} />
            <Route path="task-logs" element={<TaskLogs />} />
            <Route path="plugins" element={<PluginsList />} />
            <Route path="plugins/:name/config" element={<PluginConfig />} />

            <Route path="redemptions" element={<Redemptions />} />
            <Route path="users" element={<Users />} />
            <Route path="admins" element={<Users />} />
            <Route path="user-levels" element={<UserLevels />} />
            <Route path="user-levels/:actionId" element={<UserLevelEdit />} />
            <Route path="admin-groups" element={<AdminGroups />} />
            <Route path="admin-groups/:actionId" element={<AdminGroupEdit />} />
            <Route path="finance/recharges" element={<RechargeRecords />} />
            <Route path="finance/gifts" element={<GiftRecords />} />
            <Route path="finance/orders" element={<OrderDetails />} />
            <Route path="finance/analysis" element={<FinanceDataAnalysis />} />
            <Route path="settings" element={<Settings />} />
            <Route path="payment-settings" element={<PaymentSettings />} />
            <Route path="message-notification" element={<MessageNotification />} />
            <Route path="oauth-settings" element={<OAuthSettings />} />
            <Route path="marketing/registration-gifts" element={<RegistrationGifts />} />
            <Route path="marketing/announcements" element={<Announcements />} />
            <Route path="about" element={<SystemAbout />} />
          </Route>
        </Route>

        {/* Fallback */}
        <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
        </AdminSetupGate>
      </React.Suspense>
    </Router>
  );
};

export default App;

