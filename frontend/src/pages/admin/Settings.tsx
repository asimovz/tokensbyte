/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia 
 * @license        MIT (https://www.tokensbyte.ai/)
 */

import React, { useEffect, useRef, useState } from 'react';
import { Card, Form, Input, Button, InputNumber, message, Typography, Space, Switch, Radio, Tabs, Select, Tag, Alert, Table, Spin, Upload, Modal, DatePicker, Divider, Descriptions, Row, Col } from 'antd';
import { CloudServerOutlined, ApiOutlined, DatabaseOutlined, UploadOutlined, PlusOutlined, DeleteOutlined } from '@ant-design/icons';
import * as Icons from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useSearchParams, useNavigate } from 'react-router-dom';
import ReactQuill from 'react-quill-new';
import 'react-quill-new/dist/quill.snow.css';
import request from '../../utils/request';
import useSettingsStore from '../../store/settings';
import { enterFreshSetup } from '../../utils/freshSetup';
import { toCalendarDateParam } from '../../utils/dateRangeParams';
import dayjs from 'dayjs';

const { Text } = Typography;

/** 系统支持的所有语言定义 — 后续新增语言只需在此添加一行 */
// 火山引擎 TOS 对象存储地域配置
const TOS_REGION_GROUPS = [
  {
    group: '🇨🇳 国内版 - 火山引擎',
    regions: [
      { label: '华北2（北京）', region: 'cn-beijing', endpointExternal: 'https://tos-cn-beijing.volces.com', endpointInternal: 'https://tos-cn-beijing.ivolces.com' },
      { label: '华南1（广州）', region: 'cn-guangzhou', endpointExternal: 'https://tos-cn-guangzhou.volces.com', endpointInternal: 'https://tos-cn-guangzhou.ivolces.com' },
      { label: '华东2（上海）', region: 'cn-shanghai', endpointExternal: 'https://tos-cn-shanghai.volces.com', endpointInternal: 'https://tos-cn-shanghai.ivolces.com' },
      { label: '中国香港', region: 'cn-hongkong', endpointExternal: 'https://tos-cn-hongkong.volces.com', endpointInternal: 'https://tos-cn-hongkong.ivolces.com' },
      { label: '亚太东南（柔佛）', region: 'ap-southeast-1', endpointExternal: 'https://tos-ap-southeast-1.volces.com', endpointInternal: 'https://tos-ap-southeast-1.ivolces.com' },
      { label: '亚太东南（雅加达）', region: 'ap-southeast-3', endpointExternal: 'https://tos-ap-southeast-3.volces.com', endpointInternal: 'https://tos-ap-southeast-3.ivolces.com' },
    ]
  },
  {
    group: '🌏 海外版 - BytePlus',
    regions: [
      { label: '亚太地区（柔佛）', region: 'bp-ap-southeast-1', endpointExternal: 'https://tos-ap-southeast-1.bytepluses.com', endpointInternal: 'https://tos-ap-southeast-1.ibytepluses.com' },
      { label: '中国（香港）', region: 'bp-cn-hongkong', endpointExternal: 'https://tos-cn-hongkong.bytepluses.com', endpointInternal: 'https://tos-cn-hongkong.ibytepluses.com' },
      { label: '亚太地区（雅加达）', region: 'bp-ap-southeast-3', endpointExternal: 'https://tos-ap-southeast-3.bytepluses.com', endpointInternal: 'https://tos-ap-southeast-3.ibytepluses.com' },
      { label: '中国（北京）', region: 'bp-cn-beijing', endpointExternal: 'https://tos-cn-beijing.bytepluses.com.cn', endpointInternal: 'https://tos-cn-beijing.ibytepluses.com.cn' },
      { label: '中国（广州）', region: 'bp-cn-guangzhou', endpointExternal: 'https://tos-cn-guangzhou.bytepluses.com.cn', endpointInternal: 'https://tos-cn-guangzhou.ibytepluses.com.cn' },
      { label: '中国（上海）', region: 'bp-cn-shanghai', endpointExternal: 'https://tos-cn-shanghai.bytepluses.com.cn', endpointInternal: 'https://tos-cn-shanghai.ibytepluses.com.cn' },
    ]
  }
];
const ALL_TOS_REGIONS = TOS_REGION_GROUPS.flatMap(g => g.regions);

/** 低余额视频在途默认档（与后端对齐） */
const DEFAULT_VIDEO_INFLIGHT_TIERS: { max_available: number | null; max_inflight: number }[] = [
  { max_available: 20, max_inflight: 1 },
  { max_available: 50, max_inflight: 3 },
];

/** 后台轮询周期：与后端 RelaySettings 对齐（5–300，默认 30） */
const clampPollTickSecs = (v: unknown) => Math.min(300, Math.max(5, Number(v) || 30));

const DB_RESET_CONFIRM_TEXT = '确认清空当前数据';
const DB_RESET_COUNTDOWN_SECS = 10;

const ALL_LANGUAGES = [
  { code: 'zh', name: '简体中文', nativeName: 'Simplified Chinese', flag: '🇨🇳' },
  { code: 'zh-TW', name: '繁體中文', nativeName: 'Traditional Chinese', flag: '🇭🇰' },
  { code: 'en', name: 'English', nativeName: '英语', flag: '🇺🇸' },
  { code: 'ja', name: '日本語', nativeName: '日语', flag: '🇯🇵' },
  { code: 'ko', name: '한국어', nativeName: '韩语', flag: '🇰🇷' },
  { code: 'vi', name: 'Tiếng Việt', nativeName: '越南语', flag: '🇻🇳' },
  { code: 'fr', name: 'Français', nativeName: '法语', flag: '🇫🇷' },
  { code: 'de', name: 'Deutsch', nativeName: '德语', flag: '🇩🇪' },
  { code: 'es', name: 'Español', nativeName: '西班牙语', flag: '🇪🇸' },
  { code: 'pt', name: 'Português', nativeName: '葡萄牙语', flag: '🇧🇷' },
  { code: 'ru', name: 'Русский', nativeName: '俄语', flag: '🇷🇺' },
  { code: 'ar', name: 'العربية', nativeName: '阿拉伯语', flag: '🇸🇦' },
];

// 登录页风格选择器组件 (简洁单选切换)
const LoginStyleSelector: React.FC<{
  value?: 'split' | 'classic';
  onChange?: (val: 'split' | 'classic') => void;
}> = ({ value, onChange }) => {
  return (
    <Radio.Group value={value || 'split'} onChange={(e) => onChange?.(e.target.value)} buttonStyle="solid">
      <Radio.Button value="split">左右分栏风格</Radio.Button>
      <Radio.Button value="classic">经典居中风格</Radio.Button>
    </Radio.Group>
  );
};

const timezoneOptions = (() => {
  const timezones = Intl.supportedValuesOf ? Intl.supportedValuesOf('timeZone') : [
    'Asia/Shanghai', 'Asia/Tokyo', 'America/New_York', 'Europe/London'
  ];

  const grouped: Record<string, { value: string, label: string }[]> = {};

  timezones.forEach(tz => {
    const parts = tz.split('/');
    if (parts.length >= 2) {
      const group = parts[0];
      const city = parts.slice(1).join('/').replace(/_/g, ' ');

      const date = new Date();
      const str = date.toLocaleString('en-US', { timeZone: tz, timeZoneName: 'shortOffset' });
      const match = str.match(/(GMT|UTC)([+-]\d{1,2}(:\d{2})?)/);
      let offset = '';
      if (match) {
        offset = ` (UTC${match[2]})`;
      } else if (str.includes('GMT') || str.includes('UTC')) {
        offset = ' (UTC+0)';
      }

      if (!grouped[group]) grouped[group] = [];
      grouped[group].push({ value: tz, label: `${tz.replace(/_/g, ' ')}${offset}` });
    }
  });

  return Object.entries(grouped)
    .map(([group, options]) => ({
      label: group,
      options: options.sort((a, b) => a.label.localeCompare(b.label))
    }))
    .sort((a, b) => a.label.localeCompare(b.label));
})();

const Settings: React.FC = () => {
  const { t, i18n } = useTranslation();
  const { settings, updateStoreSettings } = useSettingsStore();
  const adminPath = settings?.site?.admin_path || 'admin1688';
  const [searchParams] = useSearchParams();
  const navigate = useNavigate();
  const tab = searchParams.get('tab') || 'basic';

  const [form] = Form.useForm();
  const enableMultilingual = Form.useWatch('enable_multilingual', form);
  const supportedLanguages: string[] = Form.useWatch('supported_languages', form) || ['zh', 'en'];
  const defaultLanguage: string = Form.useWatch('default_language', form) || 'zh';
  const logoUrl = Form.useWatch('logo', form);
  const [uploadingLogo, setUploadingLogo] = useState(false);

  const [loading, setLoading] = useState(false);
  const [serverUtcTime, setServerUtcTime] = useState<string | null>(null);
  const [basicSubTab, setBasicSubTab] = useState('site');
  const [dbSubTab, setDbSubTab] = useState('db');
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ success: boolean; message: string } | null>(null);
  const [syncDates, setSyncDates] = useState<[dayjs.Dayjs | null, dayjs.Dayjs | null]>([null, null]);
  const [syncingStats, setSyncingStats] = useState(false);
  const [dbVerifying, setDbVerifying] = useState(false);
  const [resetOpen, setResetOpen] = useState(false);
  const [resetPhrase, setResetPhrase] = useState('');
  const [resetCountdown, setResetCountdown] = useState<number | null>(null);
  const [resetting, setResetting] = useState(false);
  const resetStartedRef = useRef(false);
  const [dbInfo, setDbInfo] = useState<any>(null);
  const [dbInfoLoading, setDbInfoLoading] = useState(false);
  const [dbInfoError, setDbInfoError] = useState<string | null>(null);

  const handleManualSync = async () => {
    const [start, end] = syncDates;
    if (!start || !end) {
      message.warning('请选择要同步的日期范围');
      return;
    }
    
    setSyncingStats(true);
    try {
      const startStr = encodeURIComponent(toCalendarDateParam(start));
      const endStr = encodeURIComponent(toCalendarDateParam(end));
      const r = await (request.post(`/settings/usage-stats/sync?start_date=${startStr}&end_date=${endStr}`) as any);
      if (r.success) {
        message.success(r.message || '手动同步任务已在后台异步启动，请在后台查看日志');
        setSyncDates([null, null]);
      } else {
        message.error(r.message || '启动同步任务失败');
      }
    } catch (e) {
      console.error(e);
      message.error('启动后台同步任务失败');
    } finally {
      setSyncingStats(false);
    }
  };

  const closeResetModal = () => {
    if (resetting) return;
    resetStartedRef.current = false;
    setResetOpen(false);
    setResetPhrase('');
    setResetCountdown(null);
  };

  const handleVerifyDatabase = async () => {
    setDbVerifying(true);
    try {
      const r = await (request.post('/settings/database/verify', {}) as any);
      r.success ? message.success(r.message) : message.error(r.message);
    } catch { /* 全局拦截器已统一处理 */ }
    finally { setDbVerifying(false); }
  };

  const fetchDbInfo = async () => {
    setDbInfoLoading(true);
    setDbInfoError(null);
    try {
      const r = await (request.get('/settings/database/info') as any);
      setDbInfo(r);
    } catch (e: any) {
      setDbInfo(null);
      setDbInfoError(e?.response?.data?.error?.message || '无法读取数据库状态');
    } finally {
      setDbInfoLoading(false);
    }
  };

  const executeDatabaseReset = async () => {
    setResetting(true);
    try {
      const r = await (request.post(
        '/settings/database/initialize',
        { confirm: DB_RESET_CONFIRM_TEXT },
        { skipErrorHandler: true } as any,
      ) as any);
      if (r.success) {
        enterFreshSetup();
        return;
      }
      message.error(r.message || '清空失败');
      resetStartedRef.current = false;
      setResetCountdown(null);
    } catch (e: any) {
      const status = e?.response?.status;
      // 已发出清空：无响应或 5xx 时库可能已空，转入与全新安装相同的等待/初始化
      if (!status || status >= 500) {
        enterFreshSetup();
        return;
      }
      message.error(e?.response?.data?.error?.message || '清空失败');
      resetStartedRef.current = false;
      setResetCountdown(null);
    } finally {
      setResetting(false);
    }
  };

  const [tosNetworkType, setTosNetworkType] = useState<'external' | 'internal'>('external');
  const [userLevels, setUserLevels] = useState<any[]>([]);
  const [menuItems, setMenuItems] = useState<any[]>([]);
  const [loadingMenu, setLoadingMenu] = useState(true);

  const getTitle = () => {
    switch (tab) {
      case 'database': return '存储设置';
      default: return t('menu.basic_settings');
    }
  };

  useEffect(() => { fetchSettings(); }, [tab]);

  useEffect(() => {
    if (resetCountdown === null) return;
    if (resetCountdown === 0) {
      if (resetStartedRef.current) return;
      resetStartedRef.current = true;
      void executeDatabaseReset();
      return;
    }
    const timer = window.setTimeout(() => {
      setResetCountdown((n) => (n === null ? n : n - 1));
    }, 1000);
    return () => window.clearTimeout(timer);
  }, [resetCountdown]);

  useEffect(() => {
    if (tab === 'database' && dbSubTab === 'db') {
      void fetchDbInfo();
    }
  }, [tab, dbSubTab]);

  const fetchSettings = async () => {
    try {
      setLoadingMenu(true);
      const [response, levelsResponse, pluginsResponse] = await Promise.all([
        request.get('/settings/full') as any,
        request.get('/user_levels') as any,
        request.get('/plugins') as any
      ]);
      const { site, currency, login, registration, smtp, database: backendDatabase, agreement, storage, menu_config, relay, server_time } = response;
      if (server_time) {
        setServerUtcTime(server_time);
      }
      const defaultDatabase = { db_type: 'postgres', host: 'postgres', port: 5432, database: 'tokensapi', username: 'tokensapi', password: 'tokensapi', ssl_mode: false };
      const loadedDatabase = { ...defaultDatabase, ...backendDatabase };
      const defaultAgreement = {
        tos_mode: 'link', tos_mode_en: 'link', tos_content: '', tos_content_en: '', tos_link: '', tos_link_en: '',
        privacy_mode: 'link', privacy_mode_en: 'link', privacy_content: '', privacy_content_en: '', privacy_link: '', privacy_link_en: '',
        tos_enabled: false, privacy_enabled: false
      };
      
      const allLevels = Array.isArray(levelsResponse) ? levelsResponse : (levelsResponse.data || levelsResponse.levels || []);
      setUserLevels(allLevels);

      const activePluginsList = pluginsResponse?.plugins || [];
      const isPluginActive = (pluginName: string) => {
        const p = activePluginsList.find((item: any) => item.name === pluginName);
        return p ? p.is_enabled === 1 : false;
      };

      const defaultMenuItems = [
        { key: '/dashboard', label_zh: '系统概览', label_en: 'Dashboard', icon: 'DashboardOutlined', enabled: true, sort_order: 1, allowed_levels: 'all' },
        { key: '/playground', label_zh: '创作中心', label_en: 'Playground', icon: 'ExperimentOutlined', enabled: true, sort_order: 2, allowed_levels: 'all' },
        { key: '/playground-2026', label_zh: '创作中心2026', label_en: 'Playground 2026', icon: 'ExperimentOutlined', enabled: true, sort_order: 2.5, allowed_levels: 'all' },
        { key: '/docs', label_zh: 'API教程', label_en: 'Relay API', icon: 'RocketOutlined', enabled: true, sort_order: 3, allowed_levels: 'all' },
        { key: '/tokens', label_zh: '令牌管理', label_en: 'Tokens', icon: 'KeyOutlined', enabled: true, sort_order: 4, allowed_levels: 'all' },
        { key: '/logs', label_zh: '日志记录', label_en: 'Logs', icon: 'HistoryOutlined', enabled: true, sort_order: 5, allowed_levels: 'all' },
        { key: '/task-logs', label_zh: '任务列表', label_en: 'Task Logs', icon: 'ScheduleOutlined', enabled: true, sort_order: 6, allowed_levels: 'all' },
        { key: '/assets', label_zh: '素材管理', label_en: 'Assets', icon: 'PictureOutlined', enabled: true, sort_order: 7, allowed_levels: 'all' },
        { key: '/assets-intl', label_zh: '资产管理', label_en: 'Assets Intl', icon: 'FolderOpenOutlined', enabled: true, sort_order: 8, allowed_levels: 'all' },
        { key: '/advanced-marketing', label_zh: '高级推广', label_en: 'Advanced Marketing', icon: 'TeamOutlined', enabled: true, sort_order: 10, allowed_levels: 'all' },
        { key: '/wallet', label_zh: '我的钱包', label_en: 'Wallet', icon: 'WalletOutlined', enabled: true, sort_order: 11, allowed_levels: 'all' },
        { key: '/ark-video-monitor', label_zh: '视频监控', label_en: 'Ark Video Monitor', icon: 'VideoCameraOutlined', enabled: true, sort_order: 11.5, allowed_levels: 'all' },
        { key: '/profile', label_zh: '个人中心', label_en: 'Profile', icon: 'UserOutlined', enabled: true, sort_order: 12, allowed_levels: 'all' },
      ];

      let loadedItems = [];
      if (menu_config && menu_config.items && menu_config.items.length > 0) {
        loadedItems = menu_config.items.map((item: any) => {
          if (item.key === '/relay-api') {
            return { ...item, key: '/docs' };
          }
          return item;
        });
      } else {
        loadedItems = [...defaultMenuItems];
      }

      defaultMenuItems.forEach((defItem) => {
        if (!loadedItems.some((item: any) => item.key === defItem.key)) {
          loadedItems.push({
            ...defItem,
            sort_order: loadedItems.length + 1
          });
        }
      });

      const filteredItems = loadedItems.filter((item: any) => {
        if (item.key === '/moderation-query') return false;
        if (item.key === '/playground') return isPluginActive('playground');
        if (item.key === '/playground-2026') return isPluginActive('playground_2026');
        if (item.key === '/assets') return isPluginActive('asset_manager');
        if (item.key === '/assets-intl') return isPluginActive('asset_manager_intl');
        if (item.key === '/advanced-marketing') return isPluginActive('team_marketing');
        if (item.key === '/ark-video-monitor') return isPluginActive('volcengine_ark_monitor');

        return true;
      });

      setMenuItems(filteredItems.sort((a: any, b: any) => (a.sort_order || 0) - (b.sort_order || 0)));

      form.setFieldsValue({
        ...site,
        copyright: (site?.copyright !== undefined && site?.copyright !== null && site?.copyright !== '') ? site.copyright : '© 2026 TkeAPI. All rights reserved.',
        default_timezone: site?.default_timezone || Intl.DateTimeFormat().resolvedOptions().timeZone,
        admin_path: site?.admin_path || 'admin1688',
        ip_blacklist_enabled: site?.ip_blacklist_enabled === true,
        ip_blacklist_text: (site?.ip_blacklist || []).join('\n'),
        login_style: site?.login_style || 'split',
        login_quote: site?.login_quote || '',
        show_timezone: site?.show_timezone !== false,
        login: login || {},
        registration: {
          ...(registration || {}),
          require_bind_mobile: registration?.require_bind_mobile === true,
          require_bind_email: registration?.require_bind_email === true,
          bind_enforcement: registration?.bind_enforcement || 'all',
          enable_user_kyc: registration?.enable_user_kyc === true,
        },
        smtp,
        database: loadedDatabase,
        storage: storage || {},
        agreement: agreement || defaultAgreement,
        relay: {
          manual_poll_upstream: relay?.manual_poll_upstream !== false,
          poll_tick_secs: clampPollTickSecs(relay?.poll_tick_secs),
          video_inflight_enabled: relay?.video_inflight_enabled === true,
          asset_route_by_group: relay?.asset_route_by_group === true,
          video_inflight_tiers: Array.isArray(relay?.video_inflight_tiers) && relay.video_inflight_tiers.length > 0
            ? relay.video_inflight_tiers.map((t: any) => ({
                max_available: t.max_available ?? null,
                max_inflight: typeof t.max_inflight === 'number' ? t.max_inflight : 1,
              }))
            : DEFAULT_VIDEO_INFLIGHT_TIERS.map((t) => ({ ...t })),
        },
      });
    } catch (error) {
      console.error('Failed to fetch settings:', error);
      // 全局拦截器已统一弹出错误提示
    } finally {
      setLoadingMenu(false);
    }
  };

  const handleRepairFailedLogs = () => {
    Modal.confirm({
      title: '异常计费订单自动订正',
      content: '系统将扫描最近5000条计费状态为200成功且计费明细包含“冻结”字样、但实际上上游返回失败的异常模型订单。确认要一键退还用户余额，并扣减对应的令牌、渠道用量吗？此操作包含并发防重锁定，不会重复扣减或退费。',
      okText: '确认订正',
      cancelText: '取消',
      onOk: async () => {
        try {
          const r = await (request.post('/settings/repair-logs') as any);
          if (r.success) {
            Modal.success({
              title: '自动订正成功',
              width: 650,
              content: (
                <div>
                  <div style={{ marginBottom: 16 }}>
                    共计修复异常失败账单: <strong>{r.repaired_count}</strong> 笔，已退回用户普通余额: <strong>{r.refunded_balance}</strong>，退回赠送余额: <strong>{r.refunded_gift_balance}</strong>。同时已自动回滚对应的渠道与令牌的已用额度占用。
                  </div>
                  {r.details && r.details.length > 0 && (
                    <div style={{ maxHeight: 260, overflowY: 'auto', border: '1px solid #f0f0f0', borderRadius: 8, padding: '8px 12px', background: '#fafafa' }}>
                      <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 13 }}>
                        <thead>
                          <tr style={{ borderBottom: '2px solid #e8e8e8', color: '#595959', fontWeight: 600 }}>
                            <th style={{ padding: '6px 4px', textAlign: 'left' }}>用户 ID</th>
                            <th style={{ padding: '6px 4px', textAlign: 'left' }}>退回余额</th>
                            <th style={{ padding: '6px 4px', textAlign: 'left' }}>退回赠送</th>
                            <th style={{ padding: '6px 4px', textAlign: 'left' }}>异常原因</th>
                          </tr>
                        </thead>
                        <tbody>
                          {r.details.map((detail: any, idx: number) => (
                            <tr key={idx} style={{ borderBottom: '1px solid #e8e8e8' }}>
                              <td style={{ padding: '6px 4px', fontFamily: 'monospace', maxWidth: 130, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={detail.user_id}>{detail.user_id}</td>
                              <td style={{ padding: '6px 4px', color: '#52c41a', fontWeight: 'bold' }}>+{detail.refund_balance.toFixed(6)}</td>
                              <td style={{ padding: '6px 4px', color: '#1890ff', fontWeight: 'bold' }}>+{detail.refund_gift.toFixed(6)}</td>
                              <td style={{ padding: '6px 4px', color: '#ff4d4f', maxWidth: 160, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={detail.error_message}>{detail.error_message}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  )}
                </div>
              )
            });
          } else {
            message.error(r.message || '订正失败');
          }
        } catch (e) {
          message.error('请求接口失败');
        }
      }
    });
  };

  const handleSave = async () => {
    try {
      const values = form.getFieldsValue(true);
      let payload: any = {};

      if (tab === 'basic') {
        payload.site = {
          ...settings?.site,
          name: values.name || '', logo: values.logo || '', title: values.title || '',
          keywords: values.keywords || '', description: values.description || '',
          favicon: values.favicon || '',
          logo_title_url: (values.logo_title_url || '').trim(),
          login_title: values.login_title || '',
          login_title_url: (values.login_title_url || '').trim(),
          login_subtitle: values.login_subtitle || '',
          login_style: values.login_style || 'split',
          login_quote: values.login_quote || '',
          enable_multilingual: values.enable_multilingual !== false,
          supported_languages: values.enable_multilingual === false
            ? [values.default_language || 'zh']
            : (values.supported_languages || ['zh', 'en']),
          default_language: values.default_language || 'zh',
          enable_theme_toggle: values.enable_theme_toggle !== false,
          default_theme: values.default_theme || 'dark',
          default_timezone: values.default_timezone || settings?.site?.default_timezone || Intl.DateTimeFormat().resolvedOptions().timeZone,
          show_timezone: values.show_timezone !== false,
          copyright: values.copyright || '',
          admin_path: values.admin_path || 'admin1688',
          ip_blacklist_enabled: values.ip_blacklist_enabled === true,
          ip_blacklist: (values.ip_blacklist_text || '')
            .split('\n')
            .map((s: string) => s.trim())
            .filter(Boolean),
        };
        payload.login = {
          ...settings?.login,
          ...values.login,
        };
        payload.registration = {
          ...settings?.registration,
          ...values.registration,
          require_bind_mobile: values.registration?.require_bind_mobile === true,
          require_bind_email: values.registration?.require_bind_email === true,
          bind_enforcement: ['all', 'any', 'prompt_only'].includes(values.registration?.bind_enforcement)
            ? values.registration.bind_enforcement
            : 'all',
          enable_user_kyc: values.registration?.enable_user_kyc === true,
        };
        payload.agreement = {
          ...settings?.agreement,
          ...values.agreement,
        };
        payload.menu_config = {
          ...settings?.menu_config,
          items: menuItems.map((item, idx) => ({
            ...item,
            sort_order: idx + 1
          }))
        };
        payload.relay = {
          manual_poll_upstream: values.relay?.manual_poll_upstream !== false,
          poll_tick_secs: clampPollTickSecs(values.relay?.poll_tick_secs),
          video_inflight_enabled: values.relay?.video_inflight_enabled === true,
          asset_route_by_group: values.relay?.asset_route_by_group === true,
          video_inflight_tiers: (values.relay?.video_inflight_tiers || []).map((t: any) => ({
            max_available: t?.max_available === undefined || t?.max_available === null || t?.max_available === ''
              ? null
              : Number(t.max_available),
            max_inflight: Math.max(0, Number(t?.max_inflight) || 0),
          })),
        };
      } else if (tab === 'database') {
        if (dbSubTab === 'db') {
          return;
        }
        if (dbSubTab === 'storage' || dbSubTab === 'cleanup') {
          payload.storage = {
            ...settings?.storage,
            ...values.storage,
          };
        }
      }

      const oldAdminPath = settings?.site?.admin_path || 'admin1688';
      setLoading(true);
      const updatedSettings = await (request.post('/settings', payload) as any);
      message.success(t('settings.save_success'));
      updateStoreSettings(updatedSettings);
      if (payload.site?.title) document.title = payload.site.title;

      const newAdminPath = updatedSettings.site?.admin_path || 'admin1688';
      if (oldAdminPath !== newAdminPath && tab === 'basic') {
        const newUrl = window.location.pathname.replace(`/${oldAdminPath}`, `/${newAdminPath}`) + window.location.search;
        window.location.replace(newUrl);
      }
    } catch (error) {
      console.error('Failed to update settings:', error);
      // 全局拦截器已统一弹出错误提示
    } finally {
      setLoading(false);
    }
  };

  const GoLink: React.FC<{ to: string; text: string }> = ({ to, text }) => (
    <Button type="link" size="small" onClick={() => navigate(to)} style={{ padding: 0, height: 'auto' }}>{text}</Button>
  );


  const handleTestConnection = async () => {
    try {
      setTesting(true);
      setTestResult(null);
      const values = form.getFieldsValue(true);
      const res = await (request.post(`/settings/storage/test`, values.storage) as any);
      setTestResult(res);
    } catch (error: any) {
      setTestResult({ success: false, message: error?.response?.data?.error?.message || '测试失败' });
    } finally {
      setTesting(false);
    }
  };

  const siteSettingsContent = (
    <div style={{ maxWidth: 680 }}>
      <Form.Item label={t('settings.site_name')} name="name" rules={[{ required: true }]}><Input placeholder="Tkeapi" /></Form.Item>
      <Form.Item label="站点 Logo" extra={<Text type="secondary">支持图片链接，建议尺寸 32x32 或 40x40，留空则显示站点名称</Text>}>
        <Space.Compact style={{ width: '100%' }}>
          <Form.Item name="logo" noStyle>
            <Input placeholder="https://example.com/logo.png" />
          </Form.Item>
          <Upload
            accept="image/*"
            showUploadList={false}
            beforeUpload={async (file) => {
              if (!file.type.startsWith('image/')) {
                message.error('只支持上传图片格式的文件！');
                return Upload.LIST_IGNORE;
              }
              if (file.size > 5 * 1024 * 1024) {
                message.error('图片大小不能超过 5MB！');
                return Upload.LIST_IGNORE;
              }
              try {
                setUploadingLogo(true);
                const formData = new FormData();
                formData.append('file', file);
                formData.append('category', '站点设置');
                formData.append('remark', '站点 Logo');
                
                const res = await (request.post('/assets/upload', formData, {
                  headers: {
                    'Content-Type': 'multipart/form-data',
                    'x-plugin-ns': 'asset_manager',
                  },
                }) as Promise<any>);
                
                if (res?.asset?.file_url) {
                  form.setFieldsValue({ logo: res.asset.file_url });
                  message.success('Logo 上传成功！');
                } else {
                  message.error('上传成功，但未返回有效的图片链接');
                }
              } catch (err: any) {
                console.error(err);
                const errMsg = err.response?.data?.error?.message || err.message || '图片上传失败';
                message.error(errMsg);
              } finally {
                setUploadingLogo(false);
              }
              return Upload.LIST_IGNORE;
            }}
          >
            <Button icon={<UploadOutlined />} loading={uploadingLogo}>上传图片</Button>
          </Upload>
        </Space.Compact>
        {logoUrl && (
          <div style={{ marginTop: 6, display: 'flex', alignItems: 'center', gap: 8 }}>
            <Text type="secondary" style={{ fontSize: 12 }}>预览:</Text>
            <img src={logoUrl} alt="Logo Preview" style={{ height: 26, maxWidth: 100, objectFit: 'contain', borderRadius: 4, border: '1px solid var(--ant-color-border)', padding: '2px', background: '#fff' }} />
          </div>
        )}
      </Form.Item>
      <Form.Item
        label="控制台 Logo 标题链接"
        name="logo_title_url"
        extra={<Text type="secondary">配置后，控制台侧栏/顶栏 Logo 与站点名可点击跳转；支持 https://… 或站内 /docs，留空则不可点击</Text>}
      >
        <Input placeholder="例如：https://example.com 或 /" />
      </Form.Item>
      <Form.Item label={t('settings.site_title')} name="title" rules={[{ required: true }]}><Input placeholder="Tkeapi - LLM API Gateway" /></Form.Item>
      <Form.Item label="站点图标 (Favicon)" name="favicon" extra={<Text type="secondary">支持 .ico / .png / .svg 链接</Text>}>
        <Input placeholder="https://example.com/favicon.ico" />
      </Form.Item>
      <Form.Item label={t('settings.site_keywords')} name="keywords"><Input.TextArea rows={2} placeholder="LLM, API, Gateway" /></Form.Item>
      <Form.Item label={t('settings.site_description')} name="description"><Input.TextArea rows={3} placeholder="Description..." /></Form.Item>
      <Form.Item label="站点多语言" name="enable_multilingual" valuePropName="checked" extra={<Text type="secondary">开启后右上角显示语言切换；关闭则全站固定使用默认语言</Text>}>
        <Switch />
      </Form.Item>
      <Form.Item name="supported_languages" noStyle />
      {(() => {
        const implementedLangs = i18n.options.resources ? Object.keys(i18n.options.resources) : ['zh', 'en', 'ja', 'ko'];
        const defaultLangOptions = ALL_LANGUAGES
          .filter(l => implementedLangs.includes(l.code) && (enableMultilingual ? supportedLanguages.includes(l.code) : true))
          .map(l => ({ label: `${l.flag} ${l.name} (${l.nativeName})`, value: l.code }));

        return (
          <div style={{ border: '1px solid var(--border-custom, var(--ant-color-border-secondary, rgba(128,128,128,0.2)))', borderRadius: 6, padding: '12px 14px', marginBottom: 14, marginTop: -4 }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 8 }}>
              <Text strong style={{ fontSize: 13 }}>🌐 语言配置</Text>
              {enableMultilingual && (
                <Button size="small" type="link" style={{ padding: 0, height: 'auto' }} onClick={() => {
                  const all = ALL_LANGUAGES.map(l => l.code).filter(code => implementedLangs.includes(code));
                  form.setFieldsValue({ supported_languages: all });
                }}>全部启用已翻译语言</Button>
              )}
            </div>
            {enableMultilingual && (
              <div style={{ marginBottom: 10 }}>
                <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 6 }}>已启用的语言：</Text>
                <Space wrap size={[6, 6]}>
                  {ALL_LANGUAGES.map(lang => {
                    const isImplemented = implementedLangs.includes(lang.code);
                    const isChecked = isImplemented && supportedLanguages.includes(lang.code);
                    const isDefault = defaultLanguage === lang.code;
                    return (
                      <Tag.CheckableTag
                        key={lang.code}
                        checked={isChecked}
                        disabled={!isImplemented}
                        onChange={(checked) => {
                          if (!isImplemented) return;
                          let newLangs = [...supportedLanguages];
                          if (checked) {
                            newLangs.push(lang.code);
                          } else {
                            newLangs = newLangs.filter((l: string) => l !== lang.code);
                            if (newLangs.length === 0) newLangs = ['zh'];
                            if (defaultLanguage === lang.code) {
                              form.setFieldsValue({ default_language: newLangs[0] });
                            }
                          }
                          form.setFieldsValue({ supported_languages: newLangs });
                        }}
                        style={{
                          padding: '2px 8px',
                          fontSize: 12,
                          cursor: isImplemented ? 'pointer' : 'not-allowed',
                          opacity: isImplemented ? 1 : 0.6,
                          border: isChecked ? '1px solid transparent' : '1px dashed var(--border-custom, rgba(128,128,128,0.3))'
                        }}
                      >
                        {lang.flag} {lang.name} {isDefault && '(默认)'}
                      </Tag.CheckableTag>
                    );
                  })}
                </Space>
              </div>
            )}
            <Form.Item label="默认语言" name="default_language" style={{ marginBottom: 0 }}>
              <Select
                style={{ width: 220 }}
                options={defaultLangOptions}
                onChange={(code: string) => {
                  if (!enableMultilingual) {
                    form.setFieldsValue({ supported_languages: [code] });
                  }
                }}
              />
            </Form.Item>
          </div>
        );
      })()}
      <Form.Item
        label="站点默认时区"
        name="default_timezone"
        extra={
          <Text type="secondary">
            业务展示与统计自然日切时区。系统底层时钟固定 UTC（当前：{serverUtcTime || '—'} UTC）。
          </Text>
        }
      >
        <Select
          style={{ width: 320 }}
          showSearch
          placeholder="请选择站点默认时区（IANA）"
          options={timezoneOptions}
          filterOption={(input, option: any) =>
            (option?.label as string ?? '').toLowerCase().includes(input.toLowerCase()) ||
            (option?.value as string ?? '').toLowerCase().includes(input.toLowerCase())
          }
        />
      </Form.Item>
      <Form.Item
        label="显示时区后缀"
        name="show_timezone"
        valuePropName="checked"
        extra={<Text type="secondary">开启后在展示绝对时间时追加 (UTC+8) 等偏移标记</Text>}
      >
        <Switch />
      </Form.Item>
      <Form.Item label="允许主题切换" name="enable_theme_toggle" valuePropName="checked" extra={<Text type="secondary">开启后用户可切换亮暗模式；关闭则固定使用默认主题</Text>}>
        <Switch />
      </Form.Item>
      <Form.Item label="站点默认主题" name="default_theme">
        <Radio.Group buttonStyle="solid">
          <Radio.Button value="dark">🌙 暗色模式</Radio.Button>
          <Radio.Button value="light">☀️ 亮色模式</Radio.Button>
        </Radio.Group>
      </Form.Item>
      <Form.Item label="版权信息" name="copyright" extra={<Text type="secondary">展示在登录页及底部，留空则不显示</Text>}>
        <Input placeholder="© 2026 TkeAPI. All rights reserved." />
      </Form.Item>
    </div>
  );

  const securitySettingsContent = (
    <div style={{ maxWidth: 680 }}>
      <Form.Item
        label="管理后台访问路径"
        name="admin_path"
        rules={[
          { required: true, message: '请输入管理后台访问路径' },
          { pattern: /^[a-zA-Z0-9_\-]+$/, message: '路径仅支持字母、数字、下划线和中划线' }
        ]}
        extra={<Text type="secondary">修改后后台入口变为新路径，如 /admin1688，默认 admin1688</Text>}
      >
        <Input placeholder="admin1688" />
      </Form.Item>

      <Divider style={{ margin: '16px 0 12px' }}>注册 IP 黑名单拦截</Divider>

      <Form.Item
        label="开启注册 IP 黑名单"
        name="ip_blacklist_enabled"
        valuePropName="checked"
        extra={<Text type="secondary">开启后黑名单内的 IP 禁止发送验证码和注册</Text>}
      >
        <Switch />
      </Form.Item>

      <Form.Item
        noStyle
        shouldUpdate={(prevValues, currentValues) => prevValues.ip_blacklist_enabled !== currentValues.ip_blacklist_enabled}
      >
        {({ getFieldValue }) => {
          const enabled = getFieldValue('ip_blacklist_enabled');
          if (!enabled) return null;
          return (
            <Form.Item
              label="黑名单 IP / CIDR 网段列表"
              name="ip_blacklist_text"
              extra={<Text type="secondary">每行一个 IP 或 CIDR 网段，例如：192.168.1.100 或 10.0.0.0/8</Text>}
            >
              <Input.TextArea
                rows={4}
                placeholder={'192.168.1.100\n10.0.0.0/8'}
              />
            </Form.Item>
          );
        }}
      </Form.Item>
    </div>
  );

  const relaySettingsContent = (
    <div style={{ maxWidth: 720 }}>
      <Alert
        type="info"
        showIcon
        style={{ marginBottom: 14 }}
        title="手动轮询仅影响客户端 GET；后台自动轮询与计费不变。低余额视频限制与「余额不足」区分；保存后即时生效。"
      />
      <Form.Item
        label="手动轮询请求上游"
        name={['relay', 'manual_poll_upstream']}
        valuePropName="checked"
        extra={<Text type="secondary">开：未完成任务打上游。关：优先返回 logs 缓存；无缓存再兜底上游</Text>}
      >
        <Switch />
      </Form.Item>
      <Form.Item
        label="后台自动轮询周期"
        name={['relay', 'poll_tick_secs']}
        extra={<Text type="secondary">TaskPoller 间隔（秒），建议 15–60，范围 5–300，默认 30</Text>}
        rules={[{ required: true, message: '必填' }]}
      >
        <InputNumber min={5} max={300} step={5} addonAfter="秒" style={{ width: 180 }} />
      </Form.Item>

      <Divider style={{ margin: '16px 0 12px' }}>素材上游路由</Divider>
      <Form.Item
        label="按渠道分组推导素材上游"
        name={['relay', 'asset_route_by_group']}
        valuePropName="checked"
        extra={
          <Text type="secondary">
            关（默认）：素材上游只读【上游素材绑定】页的用户等级映射，行为与改造前一致。
            开：先按「用户等级 → 渠道分组 → 分组凭证 → 素材绑定」自动推导，等级映射退为回落与消歧通道；
            若某等级被两个带绑定的分组同时覆盖则直接报错，不会猜上游。打开前请先确认每个等级只有一个带素材绑定的分组。
          </Text>
        }
      >
        <Switch />
      </Form.Item>

      <Divider style={{ margin: '16px 0 12px' }}>低余额限制未完成视频</Divider>
      <Form.Item
        label="启用限制"
        name={['relay', 'video_inflight_enabled']}
        valuePropName="checked"
        extra={<Text type="secondary">开启后按可用额限制未完成视频路数；可用额低于填金额，路数 0=不限制</Text>}
      >
        <Switch />
      </Form.Item>
      <Form.List name={['relay', 'video_inflight_tiers']}>
        {(fields, { add, remove }) => (
          <>
            <div style={{ display: 'flex', gap: 8, marginBottom: 4, paddingRight: 32 }}>
              <Text type="secondary" style={{ fontSize: 12, width: 160 }}>可用额低于（元）</Text>
              <Text type="secondary" style={{ fontSize: 12, width: 180 }}>最大未完成视频路数</Text>
            </div>
            {fields.map((field) => (
              <div key={field.key} style={{ display: 'flex', gap: 8, alignItems: 'flex-start', marginBottom: 6 }}>
                <Form.Item
                  {...field}
                  name={[field.name, 'max_available']}
                  style={{ marginBottom: 0, width: 160 }}
                >
                  <InputNumber min={0} step={1} style={{ width: '100%' }} placeholder="其余则留空" />
                </Form.Item>
                <Form.Item
                  {...field}
                  name={[field.name, 'max_inflight']}
                  style={{ marginBottom: 0, width: 180 }}
                  rules={[{ required: true, message: '必填' }]}
                >
                  <InputNumber min={0} step={1} style={{ width: '100%' }} placeholder="0=不限制" />
                </Form.Item>
                <Button
                  type="text"
                  danger
                  icon={<DeleteOutlined />}
                  disabled={fields.length <= 1}
                  onClick={() => remove(field.name)}
                  style={{ marginTop: 2 }}
                />
              </div>
            ))}
            <Button
              type="dashed"
              size="small"
              icon={<PlusOutlined />}
              onClick={() => add({ max_available: 20, max_inflight: 1 })}
              style={{ marginTop: 2 }}
            >
              新增档
            </Button>
          </>
        )}
      </Form.List>
    </div>
  );

  const loginSettingsContent = (
    <div style={{ maxWidth: 680 }}>
      <Form.Item label="登录页标题" name="login_title" extra={<Text type="secondary">留空则使用站点名称</Text>}>
        <Input placeholder="例如：Tkeapi" />
      </Form.Item>
      <Form.Item
        label="登录页标题链接"
        name="login_title_url"
        extra={<Text type="secondary">配置后标题和 Logo 可点击跳转；留空则使用「控制台 Logo 标题链接」</Text>}
      >
        <Input placeholder="留空则使用控制台 Logo 标题链接" />
      </Form.Item>
      <Form.Item label="登录页副标题" name="login_subtitle" extra={<Text type="secondary">留空则使用默认文字</Text>}>
        <Input placeholder="例如：Next-gen LLM API Gateway" />
      </Form.Item>
      <Form.Item label="登录页风格" name="login_style" extra={<Text type="secondary">经典居中将表单直接居中；左右风格为双栏布局</Text>}>
        <LoginStyleSelector />
      </Form.Item>

      <Form.Item noStyle shouldUpdate={(prevValues, currentValues) => prevValues.login_style !== currentValues.login_style}>
        {({ getFieldValue }) => {
          const style = getFieldValue('login_style') || 'split';
          if (style !== 'split') return null;
          return (
            <Form.Item 
              label="左下角广告语" 
              name="login_quote" 
              extra={<Text type="secondary">左右风格左侧宣传语，留空使用系统默认</Text>}
            >
              <Input.TextArea rows={2} placeholder="配置登录页左侧大背景底部所展示的宣传语" />
            </Form.Item>
          );
        }}
      </Form.Item>

      <Divider style={{ margin: '16px 0 12px' }}>第三方与登录方式</Divider>

      <Form.Item label={t('settings.enable_username_login')} name={['login', 'enable_username_login']} valuePropName="checked">
        <Switch />
      </Form.Item>
      <Form.Item label={t('settings.enable_mobile_login')} name={['login', 'enable_mobile_login']} valuePropName="checked"
        extra={<Text type="secondary">{t('settings.login_hint_sms')}，<GoLink to={`/${adminPath}/message-notification`} text={t('settings.goto_settings')} /></Text>}>
        <Switch />
      </Form.Item>
      <Form.Item label={t('settings.enable_email_login')} name={['login', 'enable_email_login']} valuePropName="checked"
        extra={<Text type="secondary">{t('settings.login_hint_email')}，<GoLink to={`/${adminPath}/message-notification`} text={t('settings.goto_settings')} /></Text>}>
        <Switch />
      </Form.Item>
      <Form.Item label={t('settings.enable_wechat_login')} name={['login', 'enable_wechat_login']} valuePropName="checked"
        extra={<Text type="secondary">{t('settings.login_hint_oauth')}，<GoLink to={`/${adminPath}/oauth-settings`} text={t('settings.goto_settings')} /></Text>}>
        <Switch />
      </Form.Item>
      <Form.Item label={t('settings.enable_google_login')} name={['login', 'enable_google_login']} valuePropName="checked"
        extra={<Text type="secondary">{t('settings.login_hint_oauth')}，<GoLink to={`/${adminPath}/oauth-settings`} text={t('settings.goto_settings')} /></Text>}>
        <Switch />
      </Form.Item>
    </div>
  );

  const registrationSettingsContent = (
    <div style={{ maxWidth: 680 }}>
      <Form.Item label={t('settings.enable_username_reg')} name={['registration', 'enable_username_registration']} valuePropName="checked"><Switch /></Form.Item>
      <Form.Item label={t('settings.enable_email_reg')} name={['registration', 'enable_email_registration']} valuePropName="checked"><Switch /></Form.Item>
      <Form.Item label={t('settings.enable_mobile_registration')} name={['registration', 'enable_mobile_registration']} valuePropName="checked"
        extra={<Text type="secondary">{t('settings.login_hint_sms')}，<GoLink to={`/${adminPath}/message-notification`} text={t('settings.goto_settings')} /></Text>}>
        <Switch />
      </Form.Item>
      <Form.Item label={t('settings.enable_password_recovery')} name={['registration', 'enable_password_recovery']} valuePropName="checked"><Switch /></Form.Item>

      <Divider style={{ margin: '16px 0 12px' }}>用户实名认证 (KYC)</Divider>
      <Form.Item
        label="开启用户实名"
        name={['registration', 'enable_user_kyc']}
        valuePropName="checked"
        extra={<Text type="secondary">开启后用户可在个人中心提交实名认证</Text>}
      >
        <Switch />
      </Form.Item>

      <Divider style={{ margin: '16px 0 12px' }}>账号绑定策略</Divider>
      <Text type="secondary" style={{ display: 'block', marginBottom: 12, fontSize: 12 }}>
        开启后，未满足条件的用户登录会弹窗提醒。执行方式决定是否在创建 API 令牌时硬拦截。
      </Text>
      <Form.Item
        label="必须绑定手机"
        name={['registration', 'require_bind_mobile']}
        valuePropName="checked"
        extra={<Text type="secondary">需短信通道可用。<GoLink to={`/${adminPath}/message-notification`} text={t('settings.goto_settings')} /></Text>}
      >
        <Switch />
      </Form.Item>
      <Form.Item
        label="必须绑定邮箱"
        name={['registration', 'require_bind_email']}
        valuePropName="checked"
        extra={<Text type="secondary">需邮件通道可用。<GoLink to={`/${adminPath}/message-notification`} text={t('settings.goto_settings')} /></Text>}
      >
        <Switch />
      </Form.Item>
      <Form.Item noStyle dependencies={[['registration', 'require_bind_mobile'], ['registration', 'require_bind_email']]}>
        {({ getFieldValue, setFieldsValue }) => {
          const needMobile = getFieldValue(['registration', 'require_bind_mobile']);
          const needEmail = getFieldValue(['registration', 'require_bind_email']);
          if (!needMobile && !needEmail) return null;
          const both = !!(needMobile && needEmail);
          const mode = getFieldValue(['registration', 'bind_enforcement']) || 'all';
          if (!both && mode === 'any') {
            setTimeout(() => setFieldsValue({ registration: { ...getFieldValue('registration'), bind_enforcement: 'all' } }), 0);
          }
          return (
            <Form.Item
              label="执行方式"
              name={['registration', 'bind_enforcement']}
              initialValue="all"
            >
              <Radio.Group buttonStyle="solid">
                {both ? (
                  <>
                    <Radio.Button value="all">全部都要</Radio.Button>
                    <Radio.Button value="any">满足其一</Radio.Button>
                  </>
                ) : (
                  <Radio.Button value="all">创建令牌前必须绑定</Radio.Button>
                )}
                <Radio.Button value="prompt_only">仅弹窗提示</Radio.Button>
              </Radio.Group>
            </Form.Item>
          );
        }}
      </Form.Item>

      <Divider style={{ margin: '16px 0 12px' }}>安全策略</Divider>

      <Form.Item label={t('settings.ip_rate_limit_enabled')} name={['registration', 'ip_rate_limit_enabled']} valuePropName="checked"
        extra={<Text type="secondary">限制同一 IP 每天注册次数（手机号注册不受此限）</Text>}>
        <Switch />
      </Form.Item>
      <Form.Item noStyle dependencies={[['registration', 'ip_rate_limit_enabled']]}>
        {({ getFieldValue }) => getFieldValue(['registration', 'ip_rate_limit_enabled']) ? (
          <Form.Item label={t('settings.ip_daily_limit')} name={['registration', 'ip_daily_limit']}>
            <InputNumber min={1} max={100} addonAfter={t('settings.ip_daily_limit_unit')} style={{ width: 180 }} />
          </Form.Item>
        ) : null}
      </Form.Item>

      <Form.Item label={t('settings.email_validation_strict')} name={['registration', 'email_validation_strict']} valuePropName="checked"
        extra={<Text type="secondary">开启后邮箱 @ 前仅允许数字、字母和下划线，长度≤25</Text>}>
        <Switch />
      </Form.Item>

      <Form.Item label={t('settings.email_whitelist_enabled')} name={['registration', 'email_whitelist_enabled']} valuePropName="checked"
        extra={<Text type="secondary">开启后仅允许指定域名的邮箱注册</Text>}>
        <Switch />
      </Form.Item>
      <Form.Item noStyle dependencies={[['registration', 'email_whitelist_enabled']]}>
        {({ getFieldValue }) => getFieldValue(['registration', 'email_whitelist_enabled']) ? (
          <Form.Item label="允许的邮箱域名" name={['registration', 'email_whitelist']}>
            <Select mode="tags" placeholder={t('settings.email_whitelist_placeholder')} style={{ width: '100%' }}
              tokenSeparators={[',', ' ']} />
          </Form.Item>
        ) : null}
      </Form.Item>
    </div>
  );

  const agreementSettingsContent = (
    <div style={{ maxWidth: 780 }}>
      <div style={{ display: 'flex', gap: 32, marginBottom: 16 }}>
        <Form.Item label="启用服务条款" name={['agreement', 'tos_enabled']} valuePropName="checked" style={{ marginBottom: 0 }}>
          <Switch />
        </Form.Item>
        <Form.Item label="启用隐私协议" name={['agreement', 'privacy_enabled']} valuePropName="checked" style={{ marginBottom: 0 }}>
          <Switch />
        </Form.Item>
      </div>

      <Tabs defaultActiveKey="zh">
        <Tabs.TabPane tab="简体中文 (默认)" key="zh">
          <Text strong style={{ fontSize: 13, display: 'block', marginBottom: 8 }}>服务条款 (Terms of Service)</Text>
          <Form.Item label="显示方式" name={['agreement', 'tos_mode']}>
            <Radio.Group buttonStyle="solid">
              <Radio.Button value="link">网页链接</Radio.Button>
              <Radio.Button value="text">站内富文本</Radio.Button>
            </Radio.Group>
          </Form.Item>
          <Form.Item noStyle dependencies={[['agreement', 'tos_mode']]}>
            {({ getFieldValue }) => getFieldValue(['agreement', 'tos_mode']) === 'link' ? (
              <Form.Item label="链接地址" name={['agreement', 'tos_link']}>
                <Input placeholder="https://example.com/terms" />
              </Form.Item>
            ) : (
              <Form.Item label="条款内容" name={['agreement', 'tos_content']}>
                <ReactQuill theme="snow" style={{ height: 220, marginBottom: 42, backgroundColor: 'var(--ant-color-bg-container)', color: 'var(--ant-color-text)' }} />
              </Form.Item>
            )}
          </Form.Item>

          <Text strong style={{ fontSize: 13, display: 'block', marginTop: 20, marginBottom: 8 }}>隐私协议 (Privacy Policy)</Text>
          <Form.Item label="显示方式" name={['agreement', 'privacy_mode']}>
            <Radio.Group buttonStyle="solid">
              <Radio.Button value="link">网页链接</Radio.Button>
              <Radio.Button value="text">站内富文本</Radio.Button>
            </Radio.Group>
          </Form.Item>
          <Form.Item noStyle dependencies={[['agreement', 'privacy_mode']]}>
            {({ getFieldValue }) => getFieldValue(['agreement', 'privacy_mode']) === 'link' ? (
              <Form.Item label="链接地址" name={['agreement', 'privacy_link']}>
                <Input placeholder="https://example.com/privacy" />
              </Form.Item>
            ) : (
              <Form.Item label="协议内容" name={['agreement', 'privacy_content']}>
                <ReactQuill theme="snow" style={{ height: 220, marginBottom: 42, backgroundColor: 'var(--ant-color-bg-container)', color: 'var(--ant-color-text)' }} />
              </Form.Item>
            )}
          </Form.Item>
        </Tabs.TabPane>

        <Tabs.TabPane tab="English" key="en">
          <Text strong style={{ fontSize: 13, display: 'block', marginBottom: 8 }}>Terms of Service</Text>
          <Form.Item label="Display Mode" name={['agreement', 'tos_mode_en']}>
            <Radio.Group buttonStyle="solid">
              <Radio.Button value="link">Link URL</Radio.Button>
              <Radio.Button value="text">Rich Text</Radio.Button>
            </Radio.Group>
          </Form.Item>
          <Form.Item noStyle dependencies={[['agreement', 'tos_mode_en']]}>
            {({ getFieldValue }) => getFieldValue(['agreement', 'tos_mode_en']) === 'link' ? (
              <Form.Item label="Link URL (English)" name={['agreement', 'tos_link_en']}>
                <Input placeholder="https://example.com/en/terms" />
              </Form.Item>
            ) : (
              <Form.Item label="Content (English)" name={['agreement', 'tos_content_en']}>
                <ReactQuill theme="snow" style={{ height: 220, marginBottom: 42, backgroundColor: 'var(--ant-color-bg-container)', color: 'var(--ant-color-text)' }} />
              </Form.Item>
            )}
          </Form.Item>

          <Text strong style={{ fontSize: 13, display: 'block', marginTop: 20, marginBottom: 8 }}>Privacy Policy</Text>
          <Form.Item label="Display Mode" name={['agreement', 'privacy_mode_en']}>
            <Radio.Group buttonStyle="solid">
              <Radio.Button value="link">Link URL</Radio.Button>
              <Radio.Button value="text">Rich Text</Radio.Button>
            </Radio.Group>
          </Form.Item>
          <Form.Item noStyle dependencies={[['agreement', 'privacy_mode_en']]}>
            {({ getFieldValue }) => getFieldValue(['agreement', 'privacy_mode_en']) === 'link' ? (
              <Form.Item label="Link URL (English)" name={['agreement', 'privacy_link_en']}>
                <Input placeholder="https://example.com/en/privacy" />
              </Form.Item>
            ) : (
              <Form.Item label="Content (English)" name={['agreement', 'privacy_content_en']}>
                <ReactQuill theme="snow" style={{ height: 220, marginBottom: 42, backgroundColor: 'var(--ant-color-bg-container)', color: 'var(--ant-color-text)' }} />
              </Form.Item>
            )}
          </Form.Item>
        </Tabs.TabPane>
      </Tabs>
    </div>
  );

  const moveItem = (index: number, direction: 'up' | 'down') => {
    const newItems = [...menuItems];
    const targetIndex = direction === 'up' ? index - 1 : index + 1;
    if (targetIndex < 0 || targetIndex >= newItems.length) return;
    
    // Swap
    const temp = newItems[index];
    newItems[index] = newItems[targetIndex];
    newItems[targetIndex] = temp;
    
    setMenuItems(newItems);
  };

  const updateItem = (index: number, field: string, value: any) => {
    const newItems = [...menuItems];
    newItems[index] = {
      ...newItems[index],
      [field]: value
    };
    setMenuItems(newItems);
  };

  const menuSettingsContent = loadingMenu ? (
    <div style={{ textAlign: 'center', padding: '60px 0' }}>
      <Spin size="large" description="正在加载菜单配置..." />
    </div>
  ) : (
    <div style={{ width: '100%', overflowX: 'auto' }}>
      <div style={{ marginBottom: 16 }}>
        <Alert
          title="菜单配置说明"
          description="在此配置用户使用端（左侧菜单栏）中各个菜单的显示顺序、启用状态以及针对不同会员等级/用户组的访问可见权限。"
          type="info"
          showIcon
        />
      </div>
      <Table
        dataSource={menuItems}
        rowKey="key"
        pagination={false}
        size="middle"
        columns={[
          {
            title: '顺序',
            key: 'sort',
            width: 100,
            align: 'center',
            render: (_, __, index) => (
              <Space size="small">
                <Button
                  size="small"
                  icon={<Icons.ArrowUpOutlined />}
                  disabled={index === 0}
                  onClick={() => moveItem(index, 'up')}
                />
                <Button
                  size="small"
                  icon={<Icons.ArrowDownOutlined />}
                  disabled={index === menuItems.length - 1}
                  onClick={() => moveItem(index, 'down')}
                />
              </Space>
            ),
          },
          {
            title: '菜单图标 & 路径',
            key: 'icon_path',
            width: 200,
            render: (_, record) => {
              const IconComp = (Icons as any)[record.icon];
              return (
                <Space direction="vertical" size={2}>
                  <Space>
                    {IconComp ? <IconComp style={{ fontSize: '18px', color: '#1677ff' }} /> : <Icons.MenuOutlined style={{ fontSize: '18px' }} />}
                    <Text strong>{record.key}</Text>
                  </Space>
                  <Text type="secondary" style={{ fontSize: '12px' }}>
                    图标类名: {record.icon}
                  </Text>
                </Space>
              );
            },
          },
          {
            title: '中文名称 (Zh)',
            dataIndex: 'label_zh',
            key: 'label_zh',
            width: 180,
            render: (text, _, index) => (
              <Input
                value={text}
                onChange={(e) => updateItem(index, 'label_zh', e.target.value)}
                placeholder="中文名称"
              />
            ),
          },
          {
            title: '英文名称 (En)',
            dataIndex: 'label_en',
            key: 'label_en',
            width: 180,
            render: (text, _, index) => (
              <Input
                value={text}
                onChange={(e) => updateItem(index, 'label_en', e.target.value)}
                placeholder="英文名称"
              />
            ),
          },
          {
            title: '启用状态',
            dataIndex: 'enabled',
            key: 'enabled',
            width: 100,
            align: 'center',
            render: (checked, _, index) => (
              <Switch
                checked={checked}
                onChange={(val) => updateItem(index, 'enabled', val)}
              />
            ),
          },
          {
            title: '可见等级权限',
            dataIndex: 'allowed_levels',
            key: 'allowed_levels',
            render: (value, _, index) => {
              const selectedKeys = value === 'all' ? ['all'] : (value ? value.split(',') : []);
              return (
                <Select
                  mode="multiple"
                  style={{ width: '100%', minWidth: 200 }}
                  placeholder="选择可见等级，为空则不可见"
                  value={selectedKeys}
                  onChange={(vals: string[]) => {
                    if (vals.includes('all')) {
                      if (vals[vals.length - 1] === 'all') {
                        updateItem(index, 'allowed_levels', 'all');
                      } else {
                        const filtered = vals.filter((v: string) => v !== 'all');
                        updateItem(index, 'allowed_levels', filtered.join(','));
                      }
                    } else {
                      updateItem(index, 'allowed_levels', vals.join(','));
                    }
                  }}
                  options={[
                    { label: '全部会员等级', value: 'all' },
                    ...userLevels.map((lv) => ({
                      label: `${lv.name} (ULID: ${lv.id})`,
                      value: lv.id.toString(),
                    })),
                  ]}
                />
              );
            },
          },
        ]}
      />
    </div>
  );

  const dataCleanupContent = (
    <div style={{ maxWidth: 720 }}>
      <Alert
        type="info"
        showIcon
        style={{ marginBottom: 14 }}
        title="日志清理只清空请求/响应大字段；行归档将超期行迁入 logs_archive。火山素材清理转换素材缓存（本地+云端每日维护）。"
      />

      <Row gutter={16}>
        <Col span={8}>
          <Form.Item
            label="日志详情保留天数"
            name={['storage', 'log_retention_days']}
            extra={<Text type="secondary">0 永不清理，默认 30</Text>}
          >
            <InputNumber min={0} max={3650} style={{ width: '100%' }} addonAfter="天" placeholder="30" />
          </Form.Item>
        </Col>
        <Col span={8}>
          <Form.Item
            label="日志行归档天数"
            name={['storage', 'log_row_retention_days']}
            extra={<Text type="secondary">0 不归档，建议 90</Text>}
          >
            <InputNumber min={0} max={3650} style={{ width: '100%' }} addonAfter="天" placeholder="0" />
          </Form.Item>
        </Col>
        <Col span={8}>
          <Form.Item
            label="火山素材保留天数"
            name={['storage', 'volc_asset_retention_days']}
            extra={<Text type="secondary">转换素材缓存，默认 30</Text>}
          >
            <InputNumber min={0} max={365} style={{ width: '100%' }} addonAfter="天" placeholder="30" />
          </Form.Item>
        </Col>
      </Row>

      <Divider style={{ margin: '14px 0 16px' }} />

      <div style={{ marginBottom: 16 }}>
        <Text strong style={{ fontSize: 13, display: 'block', marginBottom: 4 }}>历史使用数据每日统计校准与补录</Text>
        <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 8 }}>
          每天凌晨自动增量同步历史日志到汇总表。若发现特定日期统计有偏差或需补录，可手动触发后台异步校准。
        </Text>
        <Space wrap>
          <DatePicker.RangePicker
            style={{ width: 260 }}
            value={syncDates}
            onChange={(val) => setSyncDates(val ? [val[0], val[1]] : [null, null])}
            disabledDate={(current) => current && current > dayjs().endOf('day')}
            placeholder={['开始日期', '结束日期']}
          />
          <Button 
            type="primary" 
            onClick={handleManualSync} 
            loading={syncingStats}
            disabled={!syncDates[0] || !syncDates[1]}
          >
            开始同步与校准
          </Button>
        </Space>
      </div>

      <Divider style={{ margin: '14px 0 16px' }} />

      <div>
        <Text strong style={{ fontSize: 13, display: 'block', marginBottom: 4 }}>异常计费订正</Text>
        <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 8 }}>
          扫描最近 5000 条「状态 200 含冻结但上游失败」的订单并退款回滚用量。未确认账单异常请勿执行。
        </Text>
        <Button danger onClick={handleRepairFailedLogs}>执行异常计费订正</Button>
      </div>
    </div>
  );

  const storageSettingsContent = (
    <div style={{ maxWidth: 760 }}>
      {testResult && (
        <Alert
          type={testResult.success ? 'success' : 'error'}
          showIcon
          style={{ marginBottom: 14 }}
          title={testResult.success ? '连接成功' : '连接失败'}
          description={testResult.message}
        />
      )}

      <Row gutter={16}>
        <Col span={12}>
          <Form.Item label="Access Key" name={['storage', 'tos_access_key']} rules={[{ required: true, message: '请输入 Access Key' }]}>
            <Input placeholder="火山引擎 Access Key" />
          </Form.Item>
        </Col>
        <Col span={12}>
          <Form.Item label="Secret Key" name={['storage', 'tos_secret_key']} rules={[{ required: true, message: '请输入 Secret Key' }]}>
            <Input.Password placeholder="火山引擎 Secret Key" />
          </Form.Item>
        </Col>

        <Col span={12}>
          <Form.Item label="数据地域" name={['storage', 'tos_region']} rules={[{ required: true, message: '请选择数据地域' }]}>
            <Select
              placeholder="选择数据地域"
              showSearch
              optionFilterProp="label"
              onChange={(value: string) => {
                const found = ALL_TOS_REGIONS.find(r => r.region === value);
                if (found) {
                  const ep = tosNetworkType === 'internal' ? found.endpointInternal : found.endpointExternal;
                  form.setFieldsValue({ storage: { ...form.getFieldValue('storage'), tos_endpoint: ep } });
                }
              }}
            >
              {TOS_REGION_GROUPS.map(g => (
                <Select.OptGroup key={g.group} label={<span style={{ fontWeight: 600, fontSize: 13 }}>{g.group}</span>}>
                  {g.regions.map(r => (
                    <Select.Option key={r.region} value={r.region} label={`${r.label} ${r.region}`}>
                      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                        <span>{r.label}</span>
                        <span style={{ color: 'var(--ant-color-text-secondary)', fontSize: 12 }}>{r.region.replace(/^bp-/, '')}</span>
                      </div>
                    </Select.Option>
                  ))}
                </Select.OptGroup>
              ))}
            </Select>
          </Form.Item>
        </Col>
        <Col span={12}>
          <Form.Item label="网络类型">
            <Radio.Group
              value={tosNetworkType}
              onChange={(e) => {
                const newType = e.target.value as 'external' | 'internal';
                setTosNetworkType(newType);
                const currentRegion = form.getFieldValue(['storage', 'tos_region']);
                if (currentRegion) {
                  const found = ALL_TOS_REGIONS.find(r => r.region === currentRegion);
                  if (found) {
                    const ep = newType === 'internal' ? found.endpointInternal : found.endpointExternal;
                    form.setFieldsValue({ storage: { ...form.getFieldValue('storage'), tos_endpoint: ep } });
                  }
                }
              }}
              optionType="button"
              buttonStyle="solid"
            >
              <Radio.Button value="external">外网</Radio.Button>
              <Radio.Button value="internal">内网</Radio.Button>
            </Radio.Group>
          </Form.Item>
        </Col>

        <Col span={12}>
          <Form.Item label="Endpoint" name={['storage', 'tos_endpoint']} rules={[{ required: true, message: '请选择地域后自动填充' }]}>
            <Input placeholder="选择地域后自动填充" />
          </Form.Item>
        </Col>
        <Col span={12}>
          <Form.Item label="Bucket 存储桶" name={['storage', 'tos_bucket']} rules={[{ required: true, message: '请输入 Bucket 名称' }]}>
            <Input placeholder="对象存储桶名称" />
          </Form.Item>
        </Col>

        <Col span={12}>
          <Form.Item label="路径前缀" name={['storage', 'tos_path_prefix']} extra={<Text type="secondary">选填，如 assets/</Text>}>
            <Input placeholder="如 assets/" />
          </Form.Item>
        </Col>
        <Col span={12}>
          <Form.Item label="自定义域名" name={['storage', 'tos_custom_domain']} extra={<Text type="secondary">选填，CDN 加速域名</Text>}>
            <Input placeholder="如 https://cdn.example.com" />
          </Form.Item>
        </Col>
      </Row>

      <Space style={{ marginTop: 4, marginBottom: 8 }}>
        <Button onClick={handleTestConnection} loading={testing}>测试 TOS 连接</Button>
      </Space>
    </div>
  );

  const dbSettingsContent = (
    <div style={{ maxWidth: 760 }}>
      <Alert
        type="info"
        showIcon
        style={{ marginBottom: 14 }}
        title="当前数据库连接与运行状态（只读）"
        description="此处展示系统当前生效的 PostgreSQL 连接与运行状态。采集只读目录与共享内存统计，打开本页或点刷新时查一次，不轮询、不扫业务大表。更改连接请修改 DATABASE_URL 或数据目录 .database_url 后重启。「初始化并清空数据库」会清空全部业务数据，随后与全新安装一样设置超级管理员。"
      />

      {dbInfoError && (
        <Alert type="warning" showIcon title={dbInfoError} style={{ marginBottom: 12 }} />
      )}

      <Spin spinning={dbInfoLoading && !dbInfo}>
        <Descriptions
          bordered
          size="small"
          column={{ xs: 1, sm: 2, md: 2 }}
          style={{ marginBottom: 16 }}
          labelStyle={{ width: '150px', fontWeight: 500 }}
        >
          <Descriptions.Item label="数据库类型">
            <Tag color="blue">PostgreSQL</Tag>
          </Descriptions.Item>
          <Descriptions.Item label="连接地址 (Host:Port)">
            <Text code>{form.getFieldValue(['database', 'host']) || 'postgres'}:{form.getFieldValue(['database', 'port']) || 5432}</Text>
          </Descriptions.Item>
          <Descriptions.Item label="数据库名称">
            <Text code>{form.getFieldValue(['database', 'database']) || 'tokensapi'}</Text>
          </Descriptions.Item>
          <Descriptions.Item label="用户名">
            <Text code>{form.getFieldValue(['database', 'username']) || 'tokensapi'}</Text>
          </Descriptions.Item>
          <Descriptions.Item label="SSL 连接">
            {form.getFieldValue(['database', 'ssl_mode']) ? <Tag color="success">已开启</Tag> : <Tag>未开启</Tag>}
          </Descriptions.Item>
          <Descriptions.Item label="数据库版本">
            {dbInfo?.server_version || '—'}
          </Descriptions.Item>
          <Descriptions.Item label="运行状态 / 运行时长">
            {dbInfo?.uptime ? <Tag color="processing">{dbInfo.uptime}</Tag> : '—'}
          </Descriptions.Item>
          <Descriptions.Item label="启动时间 (UTC)">
            {dbInfo?.started_at_utc || '—'}
          </Descriptions.Item>
          <Descriptions.Item label="数据存储大小">
            <Text strong>{dbInfo?.size_pretty || '—'}</Text>
          </Descriptions.Item>
          <Descriptions.Item label="数据表数量">
            {dbInfo?.table_count !== undefined ? `${dbInfo.table_count} 张表` : '—'}
          </Descriptions.Item>
          <Descriptions.Item label="字符编码">
            {dbInfo?.encoding || 'UTF8'}
          </Descriptions.Item>
          <Descriptions.Item label="当前连接 / 上限">
            {dbInfo?.backends !== undefined ? `${dbInfo.backends} / ${dbInfo.max_connections}` : '—'}
          </Descriptions.Item>
          <Descriptions.Item label="缓存命中率">
            {typeof dbInfo?.cache_hit_pct === 'number' ? `${dbInfo.cache_hit_pct}%` : '—'}
          </Descriptions.Item>
          <Descriptions.Item label="事务提交 / 回滚">
            {dbInfo?.xact_commit !== undefined
              ? `${Number(dbInfo.xact_commit).toLocaleString()} / ${Number(dbInfo.xact_rollback).toLocaleString()}`
              : '—'}
          </Descriptions.Item>
          <Descriptions.Item label="死锁次数">
            {dbInfo?.deadlocks !== undefined ? Number(dbInfo.deadlocks).toLocaleString() : '—'}
          </Descriptions.Item>
          <Descriptions.Item label="临时文件占用">
            {dbInfo?.temp_pretty || '—'}
          </Descriptions.Item>
          <Descriptions.Item label="计数起始 (UTC)">
            {dbInfo?.stats_reset_utc || '—'}
          </Descriptions.Item>
          <Descriptions.Item label="站点进程已运行">
            {dbInfo?.process_uptime ? <Tag color="processing">{dbInfo.process_uptime}</Tag> : '—'}
          </Descriptions.Item>
          <Descriptions.Item label="站点进程启动 (UTC)">
            {dbInfo?.process_started_at_utc || '—'}
          </Descriptions.Item>
          <Descriptions.Item label="应用连接池" span={2}>
            {dbInfo?.pool_size !== undefined
              ? `使用中 ${Number(dbInfo.pool_size) - Number(dbInfo.pool_idle || 0)}，空闲 ${dbInfo.pool_idle}，合计 ${dbInfo.pool_size}`
              : '—'}
          </Descriptions.Item>
        </Descriptions>
      </Spin>

      <Space wrap style={{ marginBottom: 8 }}>
        <Button onClick={handleVerifyDatabase} loading={dbVerifying}>测试数据库连接</Button>
        <Button onClick={() => void fetchDbInfo()} loading={dbInfoLoading}>刷新状态</Button>
        <Button
          danger
          disabled={resetting}
          onClick={() => {
            resetStartedRef.current = false;
            setResetPhrase('');
            setResetCountdown(null);
            setResetOpen(true);
          }}
        >
          初始化并清空数据库
        </Button>
      </Space>
    </div>
  );

  return (
    <Card bordered={false} title={getTitle()} style={{ borderRadius: 12 }}>
      <style>{`
        .settings-compact-form .ant-form-item {
          margin-bottom: 12px;
        }
        .settings-compact-form .ant-form-item-label {
          padding-bottom: 2px;
        }
        .settings-compact-form .ant-form-item-label > label {
          font-size: 13px;
          font-weight: 500;
        }
        .settings-compact-form .ant-form-item-extra {
          font-size: 12px;
          margin-top: 2px;
          line-height: 1.4;
        }
        .settings-compact-form .ant-card-body {
          padding: 16px 20px;
        }
        .settings-compact-form .ant-tabs-nav {
          margin-bottom: 14px;
        }
      `}</style>
      <Form className="settings-compact-form" form={form} layout="vertical" autoComplete="off"
        initialValues={{ database: { db_type: 'postgres', host: 'postgres', port: 5432, database: 'tokensapi', username: 'tokensapi', password: 'tokensapi', ssl_mode: false } }}>

        {tab === 'basic' && (
          <Tabs activeKey={basicSubTab} onChange={setBasicSubTab} items={[
            { key: 'site', label: '站点信息', children: siteSettingsContent },
            { key: 'security', label: '站点安全', children: securitySettingsContent },
            { key: 'login', label: '登录设置', children: loginSettingsContent },
            { key: 'registration', label: '注册设置', children: registrationSettingsContent },
            { key: 'agreement', label: '站点协议', children: agreementSettingsContent },
            { key: 'menu', label: '菜单配置', children: menuSettingsContent },
            { key: 'relay', label: '模型调用设置', children: relaySettingsContent },
          ]} />
        )}


        {tab === 'database' && (
          <Tabs activeKey={dbSubTab} onChange={setDbSubTab} items={[
            { key: 'db', label: '数据库设置', children: dbSettingsContent },
            { key: 'storage', label: '存储设置', children: storageSettingsContent },
            { key: 'cleanup', label: '数据清理', children: dataCleanupContent },
          ]} />
        )}

        {!(tab === 'database' && dbSubTab === 'db') && (
          <Form.Item style={{ marginTop: 16 }}>
            <Button type="primary" onClick={handleSave} loading={loading}>{t('common.save')}</Button>
          </Form.Item>
        )}
      </Form>
      <Modal
        title={resetCountdown === null ? '初始化并清空当前数据库' : '即将清空当前数据'}
        open={resetOpen}
        onCancel={closeResetModal}
        maskClosable={false}
        closable={!resetting}
        confirmLoading={resetting}
        okText={resetCountdown === null ? '开始倒计时' : undefined}
        okButtonProps={{
          danger: true,
          disabled: resetPhrase.trim() !== DB_RESET_CONFIRM_TEXT || resetting,
          style: resetCountdown !== null ? { display: 'none' } : undefined,
        }}
        cancelText="取消"
        cancelButtonProps={{ disabled: resetting }}
        onOk={() => {
          if (resetPhrase.trim() !== DB_RESET_CONFIRM_TEXT) return;
          resetStartedRef.current = false;
          setResetCountdown(DB_RESET_COUNTDOWN_SECS);
        }}
      >
        {resetCountdown === null ? (
          <div>
            <Alert
              type="error"
              showIcon
              style={{ marginBottom: 16 }}
              title="此操作会清空当前数据库的全部业务数据（用户、令牌、日志、渠道等），并重建空表结构。"
              description="连接配置不会改变。完成后将进入与全新安装相同的超级管理员设置页。此操作不可撤销。"
            />
            <div style={{ marginBottom: 8 }}>请输入「{DB_RESET_CONFIRM_TEXT}」以继续：</div>
            <Input
              value={resetPhrase}
              placeholder={DB_RESET_CONFIRM_TEXT}
              onChange={(e) => setResetPhrase(e.target.value)}
              disabled={resetting}
              autoComplete="off"
            />
          </div>
        ) : (
          <div>
            <Alert
              type="warning"
              showIcon
              title={resetting ? '正在清空当前数据库…' : `将在 ${resetCountdown} 秒后开始清空当前数据`}
              description={resetting
                ? '清空完成后将自动进入全新安装，请设置首个超级管理员。请勿关闭页面。'
                : '倒计时期间可点击取消中止。倒计时结束后将真正执行，不可再撤销。'}
            />
          </div>
        )}
      </Modal>
    </Card>
  );
};

export default Settings;
