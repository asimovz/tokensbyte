/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia 
 * @license        MIT (https://www.tokensbyte.ai/)
 */

import React, { useEffect, useState } from 'react';
import { Table, Button, Space, Tag, Modal, Form, Input, InputNumber, Switch, Segmented, message, Popconfirm, Card, Typography, AutoComplete, Grid, Tooltip, Progress, Select, Divider, TimePicker, Popover } from 'antd';
import MobileCardList, { MobileCard, CardRow, CardActions } from '../../components/MobileCardList';
import { PlusOutlined, EditOutlined, DeleteOutlined, SyncOutlined, ClearOutlined, StopOutlined, PlayCircleOutlined, SettingOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import dayjs from 'dayjs';
import request from '../../utils/request';
import { formatApiDateTime, timedisplayOffsetSuffix } from '../../utils/timedisplay';
import useSettingsStore from '../../store/settings';
import { useThemeStore } from '../../store/theme';
import type { ChannelConfig, ChannelCategory, Upstream } from '../../types';
import ChannelCategoryManager from '../../components/Channels/ChannelCategoryManager';
import {
  formatQuotaLimitDisplay,
  parseQuotaLimitInput,
  getEffectiveChannelPeriodUsed,
  validateQuotaHierarchy,
  isFiniteQuotaLimit,
} from '../../utils/quotaPeriod';

const { Title, Text } = Typography;
const { useBreakpoint } = Grid;

const UPSTREAM_SYSTEM_OPTIONS = [
  { value: '兼容', label: '兼容' },
  { value: '官方', label: '官方' },
  { value: 'newapi', label: 'newapi' },
  { value: 'akeapi', label: 'akeapi' },
  { value: '火山引擎', label: '火山引擎' },
  { value: '阿里云', label: '阿里云' },
];

type UpstreamGroupOption = { name: string; ratio: number; label: string };

function appliedChannelRate(groupRatio: number, add: number) {
  const extra = Number(add);
  return Math.max(0, Number(groupRatio) + (Number.isFinite(extra) && extra > 0 ? extra : 0));
}

function renderUpstreamSyncInline(record: ChannelConfig) {
  const system = (record.upstream_system || '').trim();
  const group = (record.upstream_group || '').trim();
  if (!system || !group) return null;
  const interval = Number(record.upstream_sync_interval_minutes) || 0;
  const add = Number(record.upstream_sync_rate_add) || 0;
  const rate = record.rate ?? 1;
  const tip = [
    `上游 ${system}`,
    `分组 ${group}（渠道倍率 ${rate}x）`,
    interval > 0 ? `每 ${interval} 分钟同步` : '不自动同步',
    add > 0 ? `同步增量 +${add}` : null,
    record.upstream_synced_at ? `上次同步 ${formatApiDateTime(record.upstream_synced_at)}` : null,
  ].filter(Boolean).join('\n');
  const tagStyle: React.CSSProperties = { margin: 0, padding: '0 5px', fontSize: 11, height: 19, lineHeight: '17px', borderRadius: 4 };
  return (
    <Tooltip title={<span style={{ whiteSpace: 'pre-line' }}>{tip}</span>}>
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-start', gap: 1, minWidth: 0 }}>
        <Tag color="blue" style={tagStyle}>{system}</Tag>
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
          <Tag style={tagStyle}>{group} {rate}x</Tag>
          {interval > 0 ? <Tag color="cyan" style={tagStyle}>每{interval}分</Tag> : null}
          {add > 0 ? <Tag color="orange" style={tagStyle}>+{add}</Tag> : null}
        </div>
      </div>
    </Tooltip>
  );
}

/** 紧凑展示在「日额度」标签后：01:00 (UTC+8) · 冷30分 */
function formatDailyResetInline(
  hour: number,
  minute: number,
  cooldown: number,
  tzSuffix: string,
): string {
  const hh = String(Math.min(23, Math.max(0, hour))).padStart(2, '0');
  const mm = String(Math.min(59, Math.max(0, minute))).padStart(2, '0');
  const cool = Math.max(0, cooldown);
  const timeText = `${hh}:${mm}${tzSuffix}`;
  return cool > 0 ? `${timeText} · 冷${cool}分` : timeText;
}

const ChannelConfigs: React.FC = () => {
  const { t } = useTranslation();
  const screens = useBreakpoint();
  const { themeMode } = useThemeStore();
  const isLight = themeMode === 'light';
  const { settings } = useSettingsStore();
  const quotaTz = settings?.site?.default_timezone || 'Asia/Shanghai';
  const [configs, setConfigs] = useState<ChannelConfig[]>([]);
  const [categories, setCategories] = useState<ChannelCategory[]>([]);
  const [loading, setLoading] = useState(true);
  const [isModalVisible, setIsModalVisible] = useState(false);
  const [editingConfig, setEditingConfig] = useState<ChannelConfig | null>(null);
  const [upstreams, setUpstreams] = useState<{ id: number, name: string }[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [searchText, setSearchText] = useState('');
  const [statusFilter, setStatusFilter] = useState<number | 'all'>('all');
  const [categoryFilter, setCategoryFilter] = useState<number | 'all' | 'unclassified'>('all');
  const [enableQuota, setEnableQuota] = useState(false);
  const [isCategoryManagerVisible, setIsCategoryManagerVisible] = useState(false);
  const [dailyResetModalOpen, setDailyResetModalOpen] = useState(false);
  const [dailyResetDraft, setDailyResetDraft] = useState({ hour: 0, minute: 0, cooldown: 0 });
  const [upstreamGroups, setUpstreamGroups] = useState<UpstreamGroupOption[]>([]);
  const [fetchingGroups, setFetchingGroups] = useState(false);
  const [syncAddEnabled, setSyncAddEnabled] = useState(false);
  const [form] = Form.useForm();
  const upstreamSystem = Form.useWatch('upstream_system', form);

  const fetchConfigs = async () => {
    setLoading(true);
    try {
      const resp = await (request.get('/channel-configs') as unknown as Promise<{ data: ChannelConfig[] }>);
      setConfigs(resp.data || []);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  };

  const fetchCategories = async () => {
    try {
      const resp = await (request.get('/channel-categories') as any);
      setCategories(Array.isArray(resp) ? resp : (resp?.data || []));
    } catch (e) {
      console.error(e);
    }
  };

  const fetchUpstreams = async () => {
    try {
      const pResp = await (request.get('/upstreams') as unknown as Promise<Upstream[]>);
      setUpstreams(pResp || []);
    } catch (e) {
      console.error(e);
    }
  };

  useEffect(() => {
    fetchConfigs();
    fetchCategories();
    fetchUpstreams();
  }, []);

  const resolveCategoryName = (categoryId?: number | null) => {
    if (!categoryId) return '';
    return categories.find(c => c.id === categoryId)?.name || '';
  };

  const activeCategories = categories.filter(c => c.is_active === 1 || c.is_active === true);

  const handleAdd = () => {
    setEditingConfig(null);
    setEnableQuota(false);
    form.resetFields();
    form.setFieldsValue({
      sort_order: 0,
      rate: 1.0,
      priority: 0,
      weight: 1,
      status: 1,
      category_id: null,
      quota_limit: -1,
      daily_quota_limit: -1,
      weekly_quota_limit: -1,
      monthly_quota_limit: -1,
      daily_reset_hour: 0,
      daily_reset_minute: 0,
      daily_reset_cooldown_minutes: 0,
      upstream_system: undefined,
      upstream_group: undefined,
      upstream_sync_interval_minutes: 0,
      upstream_sync_rate_add: 0,
    });
    setUpstreamGroups([]);
    setSyncAddEnabled(false);
    setIsModalVisible(true);
  };

  const handleEdit = (record: ChannelConfig) => {
    setEditingConfig(record);
    const q = record.quota_limit ?? -1;
    const dq = record.daily_quota_limit ?? -1;
    const wq = record.weekly_quota_limit ?? -1;
    const mq = record.monthly_quota_limit ?? -1;
    setEnableQuota(q >= 0 || dq >= 0 || wq >= 0 || mq >= 0);
    form.resetFields();
    form.setFieldsValue({
      ...record,
      status: record.status ?? 1,
      category_id: record.category_id ?? null,
      quota_limit: record.quota_limit ?? -1,
      daily_quota_limit: record.daily_quota_limit ?? -1,
      weekly_quota_limit: record.weekly_quota_limit ?? -1,
      monthly_quota_limit: record.monthly_quota_limit ?? -1,
      daily_reset_hour: record.daily_reset_hour ?? 0,
      daily_reset_minute: record.daily_reset_minute ?? 0,
      daily_reset_cooldown_minutes: record.daily_reset_cooldown_minutes ?? 0,
      upstream_system: record.upstream_system || undefined,
      upstream_group: record.upstream_group || undefined,
      upstream_sync_interval_minutes: record.upstream_sync_interval_minutes ?? 0,
      upstream_sync_rate_add: record.upstream_sync_rate_add ?? 0,
    });
    setUpstreamGroups([]);
    setSyncAddEnabled((record.upstream_sync_rate_add ?? 0) > 0);
    setIsModalVisible(true);
  };

  const handleDelete = async (id: number) => {
    try {
      await request.delete(`/channel-configs/${id}`);
      message.success(t('common.success'));
      fetchConfigs();
    } catch (e) {
      console.error(e);
    }
  };

  const handleToggleStatus = async (record: ChannelConfig) => {
    try {
      const newStatus = (record.status ?? 1) === 1 ? 0 : 1;
      await request.put(`/channel-configs/${record.id}`, { status: newStatus });
      setConfigs(prev => prev.map(c => c.id === record.id ? { ...c, status: newStatus } : c));
      message.success(newStatus === 1 ? '已启用上游渠道' : '已禁用上游渠道');
    } catch (e) {
      console.error(e);
      message.error('状态更新失败');
    }
  };

  const handleResetQuota = async (id: number) => {
    try {
      await request.post(`/channel-configs/${id}/quota/reset`);
      message.success('已清零上游预设已用额度');
      fetchConfigs();
    } catch (e) {
      console.error(e);
      message.error('清零额度失败');
    }
  };

  const openDailyResetModal = () => {
    setDailyResetDraft({
      hour: Number(form.getFieldValue('daily_reset_hour') ?? 0),
      minute: Number(form.getFieldValue('daily_reset_minute') ?? 0),
      cooldown: Number(form.getFieldValue('daily_reset_cooldown_minutes') ?? 0),
    });
    setDailyResetModalOpen(true);
  };

  const saveDailyResetModal = () => {
    form.setFieldsValue({
      daily_reset_hour: Math.min(23, Math.max(0, Number(dailyResetDraft.hour) || 0)),
      daily_reset_minute: Math.min(59, Math.max(0, Number(dailyResetDraft.minute) || 0)),
      daily_reset_cooldown_minutes: Math.max(0, Number(dailyResetDraft.cooldown) || 0),
    });
    setDailyResetModalOpen(false);
  };

  const closeConfigModal = () => {
    setDailyResetModalOpen(false);
    setIsModalVisible(false);
    setUpstreamGroups([]);
    setFetchingGroups(false);
  };

  const applyGroupToRate = (groupName?: string, addValue?: number) => {
    const name = groupName !== undefined ? groupName : form.getFieldValue('upstream_group');
    if (!name) return;
    const hit = upstreamGroups.find(g => g.name === name);
    if (!hit) return;
    const add = addValue !== undefined ? addValue : (syncAddEnabled ? Number(form.getFieldValue('upstream_sync_rate_add') || 0) : 0);
    form.setFieldsValue({ rate: appliedChannelRate(hit.ratio, add) });
  };

  const loadUpstreamGroups = async () => {
    const baseUrl = String(form.getFieldValue('base_url') || '').trim();
    const apiKey = String(form.getFieldValue('api_key') || '');
    if (!baseUrl) {
      message.warning('请先填写端点基础地址');
      return;
    }
    if (!apiKey && !editingConfig?.id) {
      message.warning('请先填写请求鉴权密钥');
      return;
    }
    setFetchingGroups(true);
    try {
      const resp = await (request.post('/channel-configs/upstream-groups', {
        config_id: editingConfig?.id,
        base_url: baseUrl,
        api_key: apiKey,
        upstream_system: 'newapi',
      }) as Promise<{ data?: UpstreamGroupOption[] }>);
      const list = resp.data || [];
      setUpstreamGroups(list);
      const current = form.getFieldValue('upstream_group');
      if (current && list.some(g => g.name === current)) {
        const add = syncAddEnabled ? Number(form.getFieldValue('upstream_sync_rate_add') || 0) : 0;
        const hit = list.find(g => g.name === current);
        if (hit) form.setFieldsValue({ rate: appliedChannelRate(hit.ratio, add) });
      }
      if (list.length === 0) {
        message.info('上游未返回分组倍率');
      } else {
        message.success(`已拉取 ${list.length} 个分组`);
      }
    } catch (e) {
      console.error(e);
    } finally {
      setFetchingGroups(false);
    }
  };

  const handleSave = async (values: any) => {
    if (submitting) return;
    setSubmitting(true);
    try {
      const payload = {
        ...values,
        base_url: values.base_url ? values.base_url.trim() : values.base_url,
        sort_order: Number(values.sort_order) || 0,
        rate: values.rate !== undefined && values.rate !== null ? Number(values.rate) : 1.0,
        priority: values.priority !== undefined && values.priority !== null ? Number(values.priority) : 0,
        weight: values.weight !== undefined && values.weight !== null ? Number(values.weight) : 1,
        status: values.status === 0 ? 0 : 1,
        category_id: values.category_id ?? null,
        quota_limit: (!enableQuota || values.quota_limit === undefined || values.quota_limit === null) ? -1 : Number(values.quota_limit),
        daily_quota_limit: (!enableQuota || values.daily_quota_limit === undefined || values.daily_quota_limit === null) ? -1 : Number(values.daily_quota_limit),
        weekly_quota_limit: (!enableQuota || values.weekly_quota_limit === undefined || values.weekly_quota_limit === null) ? -1 : Number(values.weekly_quota_limit),
        monthly_quota_limit: (!enableQuota || values.monthly_quota_limit === undefined || values.monthly_quota_limit === null) ? -1 : Number(values.monthly_quota_limit),
        daily_reset_hour: Math.min(23, Math.max(0, Number(values.daily_reset_hour) || 0)),
        daily_reset_minute: Math.min(59, Math.max(0, Number(values.daily_reset_minute) || 0)),
        daily_reset_cooldown_minutes: Math.max(0, Number(values.daily_reset_cooldown_minutes) || 0),
        upstream_system: values.upstream_system || '',
        upstream_group: values.upstream_system === 'newapi' ? (values.upstream_group || '') : '',
        upstream_sync_interval_minutes: values.upstream_system === 'newapi'
          ? Math.max(0, Number(values.upstream_sync_interval_minutes) || 0)
          : 0,
        upstream_sync_rate_add: values.upstream_system === 'newapi' && syncAddEnabled
          ? Math.max(0, Number(values.upstream_sync_rate_add) || 0)
          : 0,
      };
      if (enableQuota) {
        const hierarchyErr = validateQuotaHierarchy(payload);
        if (hierarchyErr) {
          message.error(hierarchyErr);
          setSubmitting(false);
          return;
        }
      }
      if (editingConfig) {
        // 密钥未修改（与加载时原值相同）时或未填写时不提交，防止覆盖；但如果显式清空(等于空字符串)，则提交给后端处理
        if (payload.api_key === undefined || payload.api_key === editingConfig.api_key) {
          delete payload.api_key;
        } else if (typeof payload.api_key === 'string') {
          payload.api_key = payload.api_key.trim();
        }
        await request.put(`/channel-configs/${editingConfig.id}`, payload);
        message.success(t('common.success'));
      } else {
        if (typeof payload.api_key === 'string') {
          payload.api_key = payload.api_key.trim();
        }
        await request.post('/channel-configs', payload);
        message.success(t('common.success'));
      }
      setIsModalVisible(false);
      setDailyResetModalOpen(false);
      fetchConfigs();
    } catch (e) {
      console.error(e);
    } finally {
      setSubmitting(false);
    }
  };

  const quotaRingPercent = (used: number, limit: number) => {
    if (limit < 0) return 0;
    if (limit === 0) return used > 0 ? 100 : 0;
    return Math.min(100, Math.round((used / limit) * 100));
  };

  /** 总 / 月 / 周 / 日 使用不同蓝色区分 */
  const quotaRingBlue: Record<string, string> = {
    total: '#1d4ed8', // 深蓝
    month: '#2563eb',
    week: '#3b82f6',
    day: '#60a5fa', // 浅蓝
  };

  const renderQuotaCell = (record: ChannelConfig) => {
    const used = record.quota_used || 0;
    const limit = record.quota_limit ?? -1;
    const dailyLimit = record.daily_quota_limit ?? -1;
    const weeklyLimit = record.weekly_quota_limit ?? -1;
    const monthlyLimit = record.monthly_quota_limit ?? -1;
    const { dailyUsed, weeklyUsed, monthlyUsed } = getEffectiveChannelPeriodUsed(record, quotaTz);
    const fmt = (n: number) => (Number.isInteger(n) ? String(n) : n.toFixed(6));

    const items = [
      { key: 'total', label: '总', used, limit },
      { key: 'month', label: '月', used: monthlyUsed, limit: monthlyLimit },
      { key: 'week', label: '周', used: weeklyUsed, limit: weeklyLimit },
      { key: 'day', label: '日', used: dailyUsed, limit: dailyLimit },
    ];
    const hasAnyConfigured = items.some((item) => item.limit >= 0);

    const slotWidth = 28;
    const ringSize = 24;
    const ringStroke = 5;

    const slotStyle: React.CSSProperties = {
      width: slotWidth,
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      gap: 1,
      cursor: 'default',
    };
    const labelStyle: React.CSSProperties = {
      fontSize: 9,
      color: isLight ? 'rgba(0,0,0,0.4)' : 'rgba(255,255,255,0.45)',
      lineHeight: 1,
      transform: 'scale(0.92)',
    };

    return (
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: `repeat(4, ${slotWidth}px)`,
          gap: 3,
          alignItems: 'center',
          justifyContent: 'start',
          width: 122,
        }}
      >
        {items.map((item, index) => {
          const configured = item.limit >= 0;
          // 全无限：只在「总」位展示一个 ∞，其余占位保持对齐
          const showUnlimited = !hasAnyConfigured && index === 0;
          const showRing = configured || showUnlimited;

          if (!showRing) {
            return (
              <div key={item.key} style={{ ...slotStyle, visibility: 'hidden' }} aria-hidden>
                <div style={{ width: ringSize, height: ringSize }} />
                <span style={labelStyle}>{item.label}</span>
              </div>
            );
          }

          const pct = configured ? quotaRingPercent(item.used, item.limit) : 0;
          const tip = showUnlimited
            ? `额度：${fmt(item.used)} / ∞（无限）`
            : `${item.label}额度：${fmt(item.used)} / ${fmt(Number(item.limit))}（${pct}%）`;
          const stroke = showUnlimited
            ? (isLight ? '#a1a1aa' : 'rgba(255,255,255,0.28)')
            : (pct >= 100 ? '#ef4444' : (quotaRingBlue[item.key] || '#3b82f6'));

          return (
            <Tooltip key={item.key} title={tip}>
              <div style={slotStyle}>
                <Progress
                  type="circle"
                  percent={showUnlimited ? 100 : pct}
                  size={ringSize}
                  strokeWidth={ringStroke}
                  strokeColor={stroke}
                  railColor={isLight ? '#e4e4e7' : 'rgba(255,255,255,0.12)'}
                  format={() => (
                    <span
                      style={{
                        fontSize: showUnlimited ? 9 : 8,
                        fontWeight: 600,
                        color: isLight ? 'rgba(0,0,0,0.72)' : 'rgba(255,255,255,0.88)',
                        lineHeight: 1,
                      }}
                    >
                      {showUnlimited ? '∞' : `${pct}%`}
                    </span>
                  )}
                />
                <span style={labelStyle}>{showUnlimited ? '无限' : item.label}</span>
              </div>
            </Tooltip>
          );
        })}
      </div>
    );
  };

  const renderStatusBadge = (status?: number) => {
    const active = (status ?? 1) === 1;
    return (
      <Space size={5} style={{ color: active ? '#52c41a' : '#ff4d4f' }}>
        <div style={{ width: 6, height: 6, borderRadius: '50%', backgroundColor: active ? '#52c41a' : '#ff4d4f' }} />
        <span style={{ fontSize: 12 }}>{active ? t('common.active') : t('common.disabled')}</span>
      </Space>
    );
  };

  const filteredConfigs = configs.filter(config => {
    if (statusFilter !== 'all' && (config.status ?? 1) !== statusFilter) {
      return false;
    }
    if (categoryFilter === 'unclassified') {
      if (config.category_id) return false;
    } else if (categoryFilter !== 'all') {
      if (config.category_id !== categoryFilter) return false;
    }
    if (searchText) {
      const searchLower = searchText.toLowerCase();
      const nameMatch = config.name?.toLowerCase().includes(searchLower);
      const yidMatch = config.yid?.toLowerCase().includes(searchLower);
      const providerMatch = config.provider_type?.toLowerCase().includes(searchLower);
      return nameMatch || yidMatch || providerMatch;
    }
    return true;
  });

  const renderBoundChannelsCell = (record: ChannelConfig) => {
    const count = record.bound_channel_count ?? record.bound_channels?.length ?? 0;
    if (count === 0) return <Text type="secondary" style={{ fontSize: 12 }}>未绑定</Text>;
    const channels = record.bound_channels || [];
    return (
      <Popover
        placement="bottom"
        trigger={['hover', 'click']}
        title={<div style={{ fontSize: 12, fontWeight: 600 }}>已绑定的模型渠道分组 ({count})</div>}
        content={
          <div style={{ maxHeight: 240, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 6, minWidth: 260, maxWidth: 360 }}>
            {channels.map(ch => (
              <div key={ch.id} style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8, padding: '5px 8px', borderRadius: 6, background: isLight ? '#f4f4f5' : 'rgba(255,255,255,0.06)', border: isLight ? '1px solid #e4e4e7' : '1px solid rgba(255,255,255,0.08)' }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 6, minWidth: 0, flex: 1 }}>
                  <span title={ch.name} style={{ fontSize: 12, fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', color: ch.status === 0 ? 'var(--text-secondary, #8c8c8c)' : undefined }}>{ch.name}</span>
                  {ch.status === 0 && <Tag color="error" style={{ margin: 0, fontSize: 10, padding: '0 4px', lineHeight: '14px', height: 16, borderRadius: 2 }}>已禁用</Tag>}
                  {ch.is_ha && <Tag color="purple" style={{ margin: 0, fontSize: 10, padding: '0 4px', lineHeight: '14px', height: 16, borderRadius: 2 }}>HA</Tag>}
                  {ch.has_custom_schedule && <Tag color="blue" style={{ margin: 0, fontSize: 10, padding: '0 4px', lineHeight: '14px', height: 16, borderRadius: 2 }}>独立调度</Tag>}
                </div>
              </div>
            ))}
          </div>
        }
      >
        <Tag color="processing" style={{ margin: 0, padding: '0 6px', fontSize: 11, height: 20, lineHeight: '18px', borderRadius: 4, cursor: 'pointer' }}>{count} 个分组</Tag>
      </Popover>
    );
  };

  const renderScheduleCell = (record: ChannelConfig) => {
    const customChannels = (record.bound_channels || []).filter(ch => ch.has_custom_schedule);
    return (
      <Space size={3} wrap={false} style={{ alignItems: 'center' }}>
        <Tooltip title="默认优先级"><Text type="secondary" style={{ fontSize: 12 }}>P{record.priority ?? 0}</Text></Tooltip>
        <Tooltip title="默认权重"><Text type="secondary" style={{ fontSize: 12 }}>W{record.weight ?? 1}</Text></Tooltip>
        <Tooltip title="渠道倍率"><Tag color="orange" style={{ margin: 0, padding: '0 4px', lineHeight: '16px', height: 18, fontSize: 11, borderRadius: 4 }}>{record.rate ?? 1.0}x</Tag></Tooltip>
        {customChannels.length > 0 && (
          <Popover
            placement="bottom"
            trigger={['hover', 'click']}
            title={<div style={{ fontSize: 12, fontWeight: 600 }}>分组调度详情 ({customChannels.length}/{record.bound_channel_count ?? record.bound_channels?.length ?? 0} 自定义)</div>}
            content={
              <div style={{ maxHeight: 240, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 6, minWidth: 280, maxWidth: 380 }}>
                {customChannels.map(ch => (
                  <div key={ch.id} style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8, padding: '5px 8px', borderRadius: 6, background: isLight ? 'rgba(22,119,255,0.06)' : 'rgba(22,119,255,0.14)', border: isLight ? '1px solid #91caff' : '1px solid rgba(22,119,255,0.4)' }}>
                    <span title={ch.name} style={{ fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{ch.name}</span>
                    <Space size={4} style={{ flexShrink: 0 }}>
                      <Tag color="blue" style={{ margin: 0, fontSize: 10, padding: '0 4px', lineHeight: '14px', height: 16, borderRadius: 3 }}>P{ch.effective_priority ?? record.priority ?? 0}</Tag>
                      <Tag color="blue" style={{ margin: 0, fontSize: 10, padding: '0 4px', lineHeight: '14px', height: 16, borderRadius: 3 }}>W{ch.effective_weight ?? record.weight ?? 1}</Tag>
                    </Space>
                  </div>
                ))}
              </div>
            }
          >
            <Tag color="blue" style={{ margin: 0, padding: '0 5px', lineHeight: '16px', height: 18, fontSize: 10, borderRadius: 4, cursor: 'pointer' }}>自定义({customChannels.length})</Tag>
          </Popover>
        )}
      </Space>
    );
  };

  const columns = [
    {
      title: '配置',
      key: 'name',
      width: 280,
      ellipsis: false,
      onCell: () => ({ style: { overflow: 'visible', whiteSpace: 'normal' } }),
      sorter: (a: ChannelConfig, b: ChannelConfig) => (a.name || '').localeCompare(b.name || '', 'zh'),
      render: (_: unknown, record: ChannelConfig) => {
        const sync = renderUpstreamSyncInline(record);
        return (
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, minWidth: 0 }}>
            <div style={{ minWidth: 0, display: 'flex', flexDirection: 'column', gap: 1 }}>
              <div style={{ fontWeight: 600, fontSize: 13, lineHeight: 1.25, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{record.name}</div>
              <Typography.Text keyboard style={{ color: '#1677ff', fontSize: 11, lineHeight: 1.2, fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace', width: 'fit-content' }}>
                YID {record.yid || '-'}
              </Typography.Text>
            </div>
            {sync}
          </div>
        );
      },
    },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      width: 75,
      sorter: (a: ChannelConfig, b: ChannelConfig) => (a.status ?? 1) - (b.status ?? 1),
      render: (status: number) => renderStatusBadge(status),
    },
    {
      title: '上游分类',
      dataIndex: 'category_id',
      key: 'category_id',
      width: 95,
      ellipsis: true,
      sorter: (a: ChannelConfig, b: ChannelConfig) => {
        const nameA = resolveCategoryName(a.category_id);
        const nameB = resolveCategoryName(b.category_id);
        return nameA.localeCompare(nameB, 'zh');
      },
      render: (categoryId: number | null | undefined) => {
        const name = resolveCategoryName(categoryId);
        return name ? (
          <Tag style={{ margin: 0, padding: '0 5px', fontSize: 11, height: 19, lineHeight: '17px', borderRadius: 4 }}>{name}</Tag>
        ) : <Text type="secondary" style={{ fontSize: 12 }}>未分类</Text>;
      },
    },
    {
      title: '绑定分组',
      key: 'bound_channels',
      width: 105,
      align: 'center' as const,
      sorter: (a: ChannelConfig, b: ChannelConfig) => (a.bound_channel_count ?? a.bound_channels?.length ?? 0) - (b.bound_channel_count ?? b.bound_channels?.length ?? 0),
      render: (_: unknown, record: ChannelConfig) => renderBoundChannelsCell(record),
    },
    {
      title: '服务商',
      dataIndex: 'provider_type',
      key: 'provider_type',
      width: 90,
      ellipsis: true,
      sorter: (a: ChannelConfig, b: ChannelConfig) => (a.provider_type || '').localeCompare(b.provider_type || '', 'zh'),
      render: (text: string) => <Text style={{ fontSize: 12 }}>{text || '-'}</Text>,
    },
    {
      title: '调度',
      key: 'schedule',
      width: 170,
      sorter: (a: ChannelConfig, b: ChannelConfig) => (a.priority || 0) - (b.priority || 0),
      render: (_: unknown, record: ChannelConfig) => renderScheduleCell(record),
    },
    {
      title: '额度',
      key: 'quota',
      width: 140,
      sorter: (a: ChannelConfig, b: ChannelConfig) => {
        const score = (r: ChannelConfig) => {
          const used = r.quota_used || 0;
          const limit = r.quota_limit ?? -1;
          const dailyLimit = r.daily_quota_limit ?? -1;
          const weeklyLimit = r.weekly_quota_limit ?? -1;
          const monthlyLimit = r.monthly_quota_limit ?? -1;
          const { dailyUsed, weeklyUsed, monthlyUsed } = getEffectiveChannelPeriodUsed(r, quotaTz);
          const ratios: number[] = [];
          const pushRatio = (u: number, l: number) => {
            if (l < 0) return;
            if (l === 0) {
              ratios.push(u > 0 ? Number.POSITIVE_INFINITY : 0);
              return;
            }
            ratios.push(u / l);
          };
          pushRatio(used, limit);
          pushRatio(monthlyUsed, monthlyLimit);
          pushRatio(weeklyUsed, weeklyLimit);
          pushRatio(dailyUsed, dailyLimit);
          // 未配置任何限额时按已用量排序（占比视为 0），便于与有限额项比较
          if (ratios.length === 0) return used > 0 ? used * 1e-9 : 0;
          return Math.max(...ratios);
        };
        const sa = score(a);
        const sb = score(b);
        if (sa === sb) return (a.quota_used || 0) - (b.quota_used || 0);
        return sa - sb;
      },
      render: (_: unknown, record: ChannelConfig) => renderQuotaCell(record),
    },
    {
      title: 'Base URL',
      dataIndex: 'base_url',
      key: 'base_url',
      width: 180,
      ellipsis: true,
      sorter: (a: ChannelConfig, b: ChannelConfig) => (a.base_url || '').localeCompare(b.base_url || ''),
      render: (text: string) => (
        <Tooltip title={text}>
          <Text code style={{ fontSize: 11, lineHeight: 1.2 }}>{text}</Text>
        </Tooltip>
      ),
    },
    {
      title: '排序',
      dataIndex: 'sort_order',
      key: 'sort_order',
      width: 70,
      align: 'center' as const,
      sorter: (a: ChannelConfig, b: ChannelConfig) => (a.sort_order || 0) - (b.sort_order || 0),
      render: (val: number) => <Text type="secondary" style={{ fontSize: 12 }}>{val || 0}</Text>,
    },
    {
      title: '最新更新时间',
      dataIndex: 'updated_at',
      key: 'updated_at',
      width: 150,
      ellipsis: true,
      sorter: (a: ChannelConfig, b: ChannelConfig) => {
        const ta = a.updated_at || a.created_at || '';
        const tb = b.updated_at || b.created_at || '';
        return ta.localeCompare(tb);
      },
      render: (_: unknown, record: ChannelConfig) => {
        const time = record.updated_at || record.created_at;
        return <Text type="secondary" style={{ fontSize: 12, whiteSpace: 'nowrap' }}>{time ? formatApiDateTime(time) : '-'}</Text>;
      },
    },
    {
      title: '备注',
      dataIndex: 'remark',
      key: 'remark',
      width: 100,
      ellipsis: true,
      render: (text: string) => <Text type="secondary" style={{ fontSize: 12 }}>{text || '-'}</Text>,
    },
    {
      title: t('common.actions'),
      key: 'actions',
      width: 130,
      fixed: 'right' as const,
      render: (_: unknown, record: ChannelConfig) => (
        <Space size={2} style={{ justifyContent: 'center', width: '100%' }}>
          <Tooltip title={(record.status ?? 1) === 1 ? '点击禁用' : '点击启用'}>
            <Button
              type="text"
              size="small"
              className="channel-table-action-btn"
              icon={(record.status ?? 1) === 1
                ? <PlayCircleOutlined style={{ color: '#52c41a' }} />
                : <StopOutlined style={{ color: '#ff4d4f' }} />}
              onClick={() => handleToggleStatus(record)}
            />
          </Tooltip>
          <Tooltip title="清零额度">
            <Popconfirm title="确定清零该预设的总/日/周/月已用额度吗？" onConfirm={() => handleResetQuota(record.id)}>
              <Button type="text" size="small" className="channel-table-action-btn" icon={<ClearOutlined />} />
            </Popconfirm>
          </Tooltip>
          <Tooltip title="编辑">
            <Button type="text" size="small" className="channel-table-action-btn" icon={<EditOutlined />} onClick={() => handleEdit(record)} />
          </Tooltip>
          <Tooltip title="删除">
            <Popconfirm title={t('common.confirm_delete')} onConfirm={() => handleDelete(record.id)}>
              <Button type="text" size="small" className="channel-table-action-btn" icon={<DeleteOutlined />} danger />
            </Popconfirm>
          </Tooltip>
        </Space>
      ),
    },
  ];

  return (
    <Card bordered={false}>
      <style>{`
        .channel-configs-table .ant-table,
        .channel-configs-table .ant-table-container,
        .channel-configs-table .ant-table-content,
        .channel-configs-table table {
          border-collapse: collapse !important;
          border-spacing: 0 !important;
        }
        .channel-configs-table .ant-table-thead {
          background: ${isLight ? '#f9fafb' : '#18181b'} !important;
        }
        .channel-configs-table .ant-table-thead > tr {
          height: 28px !important;
          background: ${isLight ? '#f9fafb' : '#18181b'} !important;
        }
        .channel-configs-table .ant-table-thead > tr > th,
        .channel-configs-table .ant-table-thead > tr > th.ant-table-cell {
          padding: 3px 8px !important;
          height: 28px !important;
          line-height: 20px !important;
          font-size: 12px !important;
          font-weight: 600 !important;
          background: ${isLight ? '#f9fafb' : '#18181b'} !important;
          border-bottom: 1px solid ${isLight ? 'rgba(0,0,0,0.06)' : 'rgba(255,255,255,0.08)'} !important;
          color: ${isLight ? '#64748b' : '#a1a1aa'} !important;
        }
        .channel-configs-table .ant-table-thead .ant-table-column-sorters {
          padding: 0 !important;
          margin: 0 !important;
          height: 20px !important;
          display: inline-flex !important;
          align-items: center !important;
        }
        .channel-configs-table .ant-table-thead .ant-table-column-title {
          line-height: 20px !important;
          font-size: 12px !important;
          font-weight: 600 !important;
        }
        .channel-configs-table .ant-table-thead .ant-table-column-sorter {
          margin-inline-start: 3px !important;
          font-size: 9px !important;
        }
        .channel-configs-table .ant-table-thead > tr > th::before {
          display: none !important;
        }
        .channel-configs-table .ant-table-tbody {
          margin: 0 !important;
          padding: 0 !important;
        }
        .channel-configs-table .ant-table-tbody > tr {
          margin: 0 !important;
          background: transparent !important;
        }
        .channel-configs-table .ant-table-tbody > tr > td {
          padding: 4px 8px !important;
          font-size: 12px !important;
          border-bottom: 1px solid ${isLight ? 'rgba(0,0,0,0.04)' : 'rgba(255,255,255,0.05)'} !important;
          vertical-align: middle !important;
          line-height: 1.3 !important;
        }
        .channel-configs-table .ant-table-tbody > tr:first-child > td {
          border-top: none !important;
        }
        .channel-configs-table .ant-table-measure-row,
        .channel-configs-table .ant-table-measure-row td,
        .channel-configs-table .ant-table-measure-row th,
        .channel-configs-table tr.ant-table-measure-row,
        .channel-configs-table tr.ant-table-measure-row td,
        .channel-configs-table tr.ant-table-measure-row th,
        .channel-configs-table tr.ant-table-measure-row .ant-table-cell {
          padding: 0 !important;
          height: 0 !important;
          font-size: 0 !important;
          line-height: 0 !important;
          border: none !important;
          visibility: hidden !important;
        }
        .channel-configs-table .ant-table-tbody > tr:hover > td {
          background: ${isLight ? 'rgba(0,0,0,0.02)' : 'rgba(255,255,255,0.03)'} !important;
        }
        .channel-configs-table .ant-table-pagination.ant-pagination {
          margin: 10px 0 0 0 !important;
        }
        .channel-table-action-btn {
          width: 22px !important;
          height: 22px !important;
          min-width: 22px !important;
          padding: 0 !important;
          display: inline-flex !important;
          align-items: center !important;
          justify-content: center !important;
          border-radius: 4px !important;
          font-size: 12px !important;
        }
        .channel-table-action-btn:hover {
          background: ${isLight ? 'rgba(0,0,0,0.06)' : 'rgba(255,255,255,0.08)'} !important;
        }
      `}</style>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 16, marginBottom: 24 }}>
        <Title level={4} style={{ margin: 0, fontSize: screens.xs ? 18 : 20, fontWeight: 600 }}>上游渠道配置预设</Title>
        <Space wrap>
          <Segmented
            options={[
              { label: `全部 (${configs.length})`, value: 'all' },
              { label: `激活 (${configs.filter(c => (c.status ?? 1) === 1).length})`, value: '1' },
              { label: `已禁用 (${configs.filter(c => (c.status ?? 1) === 0).length})`, value: '0' },
            ]}
            value={statusFilter === 'all' ? 'all' : statusFilter.toString()}
            onChange={(val) => setStatusFilter(val === 'all' ? 'all' : parseInt(val as string, 10))}
          />
          <Input.Search
            placeholder="名称、YID或服务商"
            allowClear
            onSearch={setSearchText}
            onChange={(e) => !e.target.value && setSearchText('')}
            style={{ width: screens.xs ? '100%' : 220 }}
          />
          <Button icon={<SyncOutlined />} onClick={fetchConfigs}>刷新</Button>
          <Button type="primary" icon={<PlusOutlined />} onClick={handleAdd}>添加配置</Button>
        </Space>
      </div>

      <div style={{
        backgroundColor: isLight ? '#fafafa' : '#141414',
        padding: '12px 16px',
        borderRadius: 8,
        marginBottom: 16,
        border: isLight ? '1px solid #e8e8e8' : '1px solid #303030',
        display: 'flex',
        flexDirection: 'column',
        gap: 10,
      }}>
        <div style={{ display: 'flex', alignItems: 'center', padding: 0 }}>
          <Text type="secondary" style={{ width: 80, flexShrink: 0, fontSize: 13 }}>上游分类</Text>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, flexGrow: 1 }}>
            {([
              { key: 'all' as const, label: '全部', count: configs.length },
              ...categories.map(c => ({
                key: c.id,
                label: c.name,
                count: configs.filter(ch => ch.category_id === c.id).length,
              })),
              {
                key: 'unclassified' as const,
                label: '未分类',
                count: configs.filter(ch => !ch.category_id).length,
              },
            ] as { key: number | 'all' | 'unclassified'; label: string; count: number }[]).map(item => {
              const selected = categoryFilter === item.key;
              return (
                <div
                  key={String(item.key)}
                  onClick={() => setCategoryFilter(item.key)}
                  style={{
                    padding: '4px 12px',
                    borderRadius: 16,
                    fontSize: 14,
                    backgroundColor: selected ? '#1677ff' : (isLight ? '#f0f0f0' : '#1d1d1d'),
                    color: selected ? '#fff' : (isLight ? 'rgba(0,0,0,0.65)' : 'rgba(255,255,255,0.65)'),
                    border: isLight ? '1px solid #d9d9d9' : '1px solid #303030',
                    cursor: 'pointer',
                    display: 'inline-flex',
                    alignItems: 'center',
                    gap: 6,
                    transition: 'all 0.2s',
                  }}
                >
                  {item.label}
                  <span style={{ opacity: 0.6 }}>{item.count}</span>
                </div>
              );
            })}
            <Tooltip title={t('common.manage', '管理')}>
              <Button
                type="text"
                size="small"
                icon={<SettingOutlined style={{ color: '#1677ff' }} />}
                onClick={() => setIsCategoryManagerVisible(true)}
                style={{ marginLeft: 8 }}
              />
            </Tooltip>
          </div>
        </div>
      </div>

      {screens.xs ? (
        <MobileCardList
          dataSource={filteredConfigs}
          loading={loading}
          rowKey="id"
          renderCard={(record: ChannelConfig) => {
            const sync = renderUpstreamSyncInline(record);
            return (
            <MobileCard
              title={record.name}
              extra={<Typography.Text keyboard style={{ color: '#1677ff' }}>{record.yid || '-'}</Typography.Text>}
            >
              {sync ? <CardRow label="上游同步">{sync}</CardRow> : null}
              <CardRow label="状态">{renderStatusBadge(record.status)}</CardRow>
              <CardRow label="上游分类">{resolveCategoryName(record.category_id) || '未分类'}</CardRow>
              <CardRow label="服务商广场展示">{record.provider_type || '-'}</CardRow>
              <CardRow label="Base URL"><Text code style={{ fontSize: 12 }}>{record.base_url}</Text></CardRow>
              <CardRow label="额度">{renderQuotaCell(record)}</CardRow>
              <CardRow label="最新更新时间">
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {record.updated_at || record.created_at ? formatApiDateTime(record.updated_at || record.created_at) : '-'}
                </Text>
              </CardRow>
              <CardRow label="备注">{record.remark || '-'}</CardRow>
              <CardActions>
                <Tooltip title={(record.status ?? 1) === 1 ? '点击禁用' : '点击启用'}>
                  <Button
                    type="text"
                    size="small"
                    icon={(record.status ?? 1) === 1
                      ? <PlayCircleOutlined style={{ color: '#52c41a' }} />
                      : <StopOutlined style={{ color: '#ff4d4f' }} />}
                    onClick={() => handleToggleStatus(record)}
                  />
                </Tooltip>
                <Tooltip title="清零额度">
                  <Popconfirm title="确定清零已用额度吗？" onConfirm={() => handleResetQuota(record.id)}>
                    <Button type="text" size="small" icon={<ClearOutlined />} />
                  </Popconfirm>
                </Tooltip>
                <Button type="text" size="small" icon={<EditOutlined />} onClick={() => handleEdit(record)} />
                <Popconfirm title={t('common.confirm_delete')} onConfirm={() => handleDelete(record.id)}>
                  <Button type="text" size="small" icon={<DeleteOutlined />} danger />
                </Popconfirm>
              </CardActions>
            </MobileCard>
            );
          }}
        />
      ) : (
        <Table
          className="channel-configs-table compact-table"
          size="small"
          dataSource={filteredConfigs}
          columns={columns}
          rowKey="id"
          loading={loading}
          pagination={{ pageSize: 15, showTotal: (total) => `共 ${total} 条` }}
          scroll={{ x: 1370 }}
        />
      )}

      <Modal
        title={editingConfig ? "编辑上游渠道配置" : "添加上游渠道配置"}
        open={isModalVisible}
        onCancel={closeConfigModal}
        onOk={() => form.submit()}
        confirmLoading={submitting}
        width={560}
        styles={{ body: { paddingTop: 12, paddingBottom: 4 } }}
      >
        <Form
          form={form}
          layout="vertical"
          size="small"
          onFinish={handleSave}
          autoComplete="off"
          style={{ marginBottom: 0 }}
        >
          <div style={{ display: 'flex', gap: 12 }}>
            <Form.Item
              name="name"
              label={
                <Space size={6}>
                  <span>配置名称</span>
                  {editingConfig?.yid && (
                    <Text type="secondary" style={{ fontSize: 11 }}>YID: {editingConfig.yid}</Text>
                  )}
                </Space>
              }
              rules={[{ required: true, message: '请输入配置名称' }]}
              style={{ flex: 1.4, marginBottom: 10 }}
            >
              <Input placeholder="例如：OpenAI 官方渠道" autoComplete="off" />
            </Form.Item>
            <Form.Item
              name="provider_type"
              label="服务商类型(模型广场展示)"
              style={{ flex: 1, marginBottom: 10 }}
            >
              <Input placeholder="如: custom / openai" autoComplete="off" />
            </Form.Item>
          </div>

          <div style={{ display: 'flex', gap: 12, alignItems: 'flex-start' }}>
            <Form.Item name="category_id" label="上游分类" style={{ flex: 1, marginBottom: 10 }}>
              <Select
                allowClear
                placeholder="选择分类"
                options={activeCategories.map(c => ({ label: c.name, value: c.id }))}
                dropdownRender={(menu) => (
                  <>
                    {menu}
                    <Divider style={{ margin: '6px 0' }} />
                    <Button
                      type="link"
                      size="small"
                      icon={<SettingOutlined />}
                      onClick={() => setIsCategoryManagerVisible(true)}
                      style={{ width: '100%' }}
                    >
                      管理分类
                    </Button>
                  </>
                )}
              />
            </Form.Item>
            <Form.Item
              name="sort_order"
              label={
                <Tooltip title="数字越大在页面中越靠前">
                  <span>页面排序</span>
                </Tooltip>
              }
              style={{ width: 96, marginBottom: 10 }}
            >
              <InputNumber placeholder="0" style={{ width: '100%' }} />
            </Form.Item>
          </div>

          <div style={{ fontSize: 12, color: 'var(--text-secondary)', marginBottom: 6 }}>
            优先级 / 权重请在分组内设置；此处的值仅作兜底，只有未配置组内策略的分组才会跟随。
          </div>

          <div style={{ display: 'flex', gap: 12 }}>
            <Form.Item name="rate" label="渠道倍率" rules={[{ required: true }]} style={{ flex: 1, marginBottom: 10 }}>
              <InputNumber min={0} step={0.1} placeholder="1.0" style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="priority" label="优先级" rules={[{ required: true }]} style={{ flex: 1, marginBottom: 10 }}>
              <InputNumber min={0} placeholder="0" style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="weight" label="权重" rules={[{ required: true }]} style={{ flex: 1, marginBottom: 10 }}>
              <InputNumber min={1} placeholder="1" style={{ width: '100%' }} />
            </Form.Item>
          </div>

          <Form.Item label="额度限额" style={{ marginBottom: 10 }}>
            <Switch
              checked={enableQuota}
              onChange={(checked) => {
                setEnableQuota(checked);
                form.setFieldsValue({
                  quota_limit: -1,
                  daily_quota_limit: -1,
                  weekly_quota_limit: -1,
                  monthly_quota_limit: -1,
                });
              }}
              checkedChildren="已开启限额"
              unCheckedChildren="默认不限额"
            />
          </Form.Item>

          <Form.Item name="daily_reset_hour" hidden initialValue={0}><InputNumber /></Form.Item>
          <Form.Item name="daily_reset_minute" hidden initialValue={0}><InputNumber /></Form.Item>
          <Form.Item name="daily_reset_cooldown_minutes" hidden initialValue={0}><InputNumber /></Form.Item>

          {enableQuota && (
            <Form.Item
              noStyle
              shouldUpdate={(prev, curr) =>
                prev.quota_limit !== curr.quota_limit ||
                prev.daily_quota_limit !== curr.daily_quota_limit ||
                prev.weekly_quota_limit !== curr.weekly_quota_limit ||
                prev.monthly_quota_limit !== curr.monthly_quota_limit ||
                prev.daily_reset_hour !== curr.daily_reset_hour ||
                prev.daily_reset_minute !== curr.daily_reset_minute ||
                prev.daily_reset_cooldown_minutes !== curr.daily_reset_cooldown_minutes
              }
            >
              {() => {
                const quotaDeps = ['quota_limit', 'daily_quota_limit', 'weekly_quota_limit', 'monthly_quota_limit'] as const;
                const fieldValidator = (field: typeof quotaDeps[number]) => ({
                  validator: async (_: unknown, value: number | null) => {
                    if (value != null && Number(value) < -1) {
                      throw new Error('额度不能小于 -1');
                    }
                    const latest = {
                      ...form.getFieldsValue([...quotaDeps]),
                      [field]: value,
                    };
                    const total = latest.quota_limit;
                    const day = latest.daily_quota_limit;
                    const week = latest.weekly_quota_limit;
                    const month = latest.monthly_quota_limit;

                    const fail = (msg: string) => {
                      throw new Error(msg);
                    };

                    if (field === 'daily_quota_limit' && isFiniteQuotaLimit(day)) {
                      if (isFiniteQuotaLimit(week) && day > week) fail('日额度不能大于周额度');
                      if (isFiniteQuotaLimit(month) && day > month) fail('日额度不能大于月额度');
                      if (isFiniteQuotaLimit(total) && day > total) fail('日额度不能大于总额度');
                    }
                    if (field === 'weekly_quota_limit' && isFiniteQuotaLimit(week)) {
                      if (isFiniteQuotaLimit(month) && week > month) fail('周额度不能大于月额度');
                      if (isFiniteQuotaLimit(total) && week > total) fail('周额度不能大于总额度');
                      if (isFiniteQuotaLimit(day) && day > week) fail('周额度不能小于日额度');
                    }
                    if (field === 'monthly_quota_limit' && isFiniteQuotaLimit(month)) {
                      if (isFiniteQuotaLimit(total) && month > total) fail('月额度不能大于总额度');
                      if (isFiniteQuotaLimit(week) && week > month) fail('月额度不能小于周额度');
                      if (isFiniteQuotaLimit(day) && day > month) fail('月额度不能小于日额度');
                    }
                    if (field === 'quota_limit' && isFiniteQuotaLimit(total)) {
                      if (isFiniteQuotaLimit(month) && month > total) fail('总额度不能小于月额度');
                      if (isFiniteQuotaLimit(week) && week > total) fail('总额度不能小于周额度');
                      if (isFiniteQuotaLimit(day) && day > total) fail('总额度不能小于日额度');
                    }
                  },
                });
                const quotaItemStyle: React.CSSProperties = { marginBottom: 10 };
                const resetHour = Number(form.getFieldValue('daily_reset_hour') ?? 0);
                const resetMinute = Number(form.getFieldValue('daily_reset_minute') ?? 0);
                const resetCooldown = Number(form.getFieldValue('daily_reset_cooldown_minutes') ?? 0);
                const tzSuffix = timedisplayOffsetSuffix(quotaTz);
                const resetInline = formatDailyResetInline(resetHour, resetMinute, resetCooldown, tzSuffix);
                const hh = String(Math.min(23, Math.max(0, resetHour))).padStart(2, '0');
                const mm = String(Math.min(59, Math.max(0, resetMinute))).padStart(2, '0');
                const resetTooltip = resetCooldown > 0
                  ? `每天 ${hh}:${mm}${tzSuffix} 起，再冷却 ${resetCooldown} 分钟后清零日已用`
                  : `每天 ${hh}:${mm}${tzSuffix} 清零日已用`;
                return (
                  <div
                    style={{
                      display: 'grid',
                      gridTemplateColumns: '1fr 1fr',
                      columnGap: 12,
                      marginBottom: 4,
                    }}
                  >
                    <Form.Item
                      name="quota_limit"
                      label="总额度"
                      initialValue={-1}
                      style={quotaItemStyle}
                      dependencies={quotaDeps as unknown as string[]}
                      validateTrigger={['onChange', 'onBlur']}
                      rules={[fieldValidator('quota_limit')]}
                    >
                      <InputNumber
                        min={-1}
                        style={{ width: '100%' }}
                        formatter={formatQuotaLimitDisplay}
                        parser={parseQuotaLimitInput}
                      />
                    </Form.Item>
                    <Form.Item
                      name="monthly_quota_limit"
                      label="月额度"
                      initialValue={-1}
                      style={quotaItemStyle}
                      dependencies={quotaDeps as unknown as string[]}
                      validateTrigger={['onChange', 'onBlur']}
                      rules={[fieldValidator('monthly_quota_limit')]}
                    >
                      <InputNumber
                        min={-1}
                        style={{ width: '100%' }}
                        formatter={formatQuotaLimitDisplay}
                        parser={parseQuotaLimitInput}
                      />
                    </Form.Item>
                    <Form.Item
                      name="weekly_quota_limit"
                      label="周额度"
                      initialValue={-1}
                      style={quotaItemStyle}
                      dependencies={quotaDeps as unknown as string[]}
                      validateTrigger={['onChange', 'onBlur']}
                      rules={[fieldValidator('weekly_quota_limit')]}
                    >
                      <InputNumber
                        min={-1}
                        style={{ width: '100%' }}
                        formatter={formatQuotaLimitDisplay}
                        parser={parseQuotaLimitInput}
                      />
                    </Form.Item>
                    <Form.Item
                      name="daily_quota_limit"
                      label={
                        <span
                          style={{
                            display: 'inline-flex',
                            alignItems: 'center',
                            gap: 4,
                            maxWidth: '100%',
                            verticalAlign: 'middle',
                          }}
                        >
                          <span>日额度</span>
                          <Tooltip title={resetTooltip}>
                            <Text
                              type="secondary"
                              style={{
                                fontSize: 11,
                                fontWeight: 400,
                                lineHeight: 1.2,
                                maxWidth: 148,
                                overflow: 'hidden',
                                textOverflow: 'ellipsis',
                                whiteSpace: 'nowrap',
                              }}
                            >
                              {resetInline}
                            </Text>
                          </Tooltip>
                          <Tooltip title="配置日额度刷新时间与冷却">
                            <Button
                              type="link"
                              size="small"
                              icon={<SettingOutlined />}
                              aria-label="配置日额度刷新"
                              onClick={(e) => {
                                e.preventDefault();
                                e.stopPropagation();
                                openDailyResetModal();
                              }}
                              style={{
                                padding: 0,
                                width: 18,
                                height: 18,
                                minWidth: 18,
                                lineHeight: '18px',
                                flexShrink: 0,
                              }}
                            />
                          </Tooltip>
                        </span>
                      }
                      initialValue={-1}
                      style={quotaItemStyle}
                      dependencies={quotaDeps as unknown as string[]}
                      validateTrigger={['onChange', 'onBlur']}
                      rules={[fieldValidator('daily_quota_limit')]}
                    >
                      <InputNumber
                        min={-1}
                        style={{ width: '100%' }}
                        formatter={formatQuotaLimitDisplay}
                        parser={parseQuotaLimitInput}
                      />
                    </Form.Item>
                  </div>
                );
              }}
            </Form.Item>
          )}

          <Form.Item name="base_url" label="端点基础地址 (Base URL)" rules={[{ required: true }]} style={{ marginBottom: 10 }}>
            <AutoComplete
              options={[
                { value: 'https://ark.cn-beijing.volces.com', label: '火山方舟 (https://ark.cn-beijing.volces.com)' },
                { value: 'https://ark.ap-southeast.bytepluses.com/api/v3', label: 'BytePlus(ap-southeast-1) (https://ark.ap-southeast.bytepluses.com/api/v3)' },
                { value: 'https://ark.eu-west.bytepluses.com/api/v3', label: 'BytePlus(eu-west-1) (https://ark.eu-west.bytepluses.com/api/v3)' },
                { value: 'https://api-beijing.klingai.com', label: '可灵 (https://api-beijing.klingai.com)' },
                { value: 'https://dashscope.aliyuncs.com', label: '阿里百炼 (https://dashscope.aliyuncs.com)' },
                { value: 'https://vod.tencentcloudapi.com', label: '腾讯云 VOD AIGC (https://vod.tencentcloudapi.com)' },
                { value: 'https://visual.volcengineapi.com', label: '即梦AI/火山CV (https://visual.volcengineapi.com)' },
              ]}
              placeholder="选择预设或自由输入"
              filterOption={(inputValue, option) =>
                String(option?.label || '').toUpperCase().indexOf(inputValue.toUpperCase()) !== -1 ||
                String(option?.value || '').toUpperCase().indexOf(inputValue.toUpperCase()) !== -1
              }
            />
          </Form.Item>
          <Form.Item
            name="api_key"
            label={
              <Tooltip
                title={
                  editingConfig
                    ? '保持不变直接保存即可，输入新值将覆盖旧密钥'
                    : '可灵新协议(kling_video)：官方 API Key 直传 Bearer；可灵旧协议(kling)：access_key:secret_key（自动 JWT）；腾讯云 VOD：SecretId:SecretKey:SubAppId；即梦AI：AccessKeyID:SecretAccessKey；其他：sk-xxx'
                }
              >
                <span>请求鉴权密钥 (API Key)</span>
              </Tooltip>
            }
            style={{ marginBottom: 10 }}
          >
            <Input.Password
              autoComplete="new-password"
              placeholder={editingConfig ? '保持当前密钥或输入新值覆盖' : 'API Key / sk-... / access_key:secret_key'}
            />
          </Form.Item>
          <Form.Item name="upstream_system" label="上游系统" style={{ marginBottom: 10 }}>
            <Select
              allowClear
              placeholder="可选"
              options={UPSTREAM_SYSTEM_OPTIONS}
            />
          </Form.Item>
          {upstreamSystem === 'newapi' && (
            <>
              <div style={{ display: 'flex', gap: 12, alignItems: 'flex-start' }}>
                <Form.Item
                  name="upstream_group"
                  label="同步分组倍率"
                  extra="选中后写入上方「渠道倍率」"
                  style={{ flex: 1, marginBottom: 10 }}
                >
                  <Select
                    allowClear
                    showSearch
                    placeholder={upstreamGroups.length ? '选择要同步的分组' : '先拉取分组倍率'}
                    optionFilterProp="label"
                    onChange={(value) => applyGroupToRate(value || undefined)}
                    options={
                      upstreamGroups.length
                        ? upstreamGroups.map(g => ({
                            value: g.name,
                            label: `${g.name}  ${g.ratio}x${g.label && g.label !== g.name ? `  ${g.label}` : ''}`,
                          }))
                        : (form.getFieldValue('upstream_group')
                            ? [{ value: form.getFieldValue('upstream_group'), label: form.getFieldValue('upstream_group') }]
                            : [])
                    }
                  />
                </Form.Item>
                <Form.Item label=" " style={{ marginBottom: 10, width: 118 }}>
                  <Button
                    icon={<SyncOutlined spin={fetchingGroups} />}
                    loading={fetchingGroups}
                    onClick={loadUpstreamGroups}
                    style={{ width: '100%' }}
                  >
                    拉取分组
                  </Button>
                </Form.Item>
              </div>
              <div style={{ display: 'flex', gap: 12 }}>
                <Form.Item
                  name="upstream_sync_interval_minutes"
                  label="同步间隔（分钟）"
                  extra="0 为不自动同步"
                  style={{ flex: 1, marginBottom: 10 }}
                >
                  <InputNumber min={0} max={10080} precision={0} placeholder="0" style={{ width: '100%' }} />
                </Form.Item>
                <Form.Item label="同步后叠加增量" style={{ flex: 1, marginBottom: 10 }}>
                  <Space.Compact style={{ width: '100%' }}>
                    <div style={{ display: 'flex', alignItems: 'center', paddingRight: 8 }}>
                      <Switch
                        size="small"
                        checked={syncAddEnabled}
                        onChange={(checked) => {
                          setSyncAddEnabled(checked);
                          const nextAdd = checked ? Number(form.getFieldValue('upstream_sync_rate_add') || 0) : 0;
                          if (!checked) form.setFieldsValue({ upstream_sync_rate_add: 0 });
                          applyGroupToRate(undefined, nextAdd);
                        }}
                      />
                    </div>
                    <Form.Item name="upstream_sync_rate_add" noStyle>
                      <InputNumber
                        min={0}
                        step={0.01}
                        disabled={!syncAddEnabled}
                        placeholder="0"
                        style={{ width: '100%' }}
                        onChange={(value) => applyGroupToRate(undefined, Number(value) || 0)}
                      />
                    </Form.Item>
                  </Space.Compact>
                </Form.Item>
              </div>
            </>
          )}
          <Form.Item
            name="remark"
            label={
              <Tooltip title="记录渠道归属、适用场景等，方便查阅">
                <span>备注说明</span>
              </Tooltip>
            }
            style={{ marginBottom: 10 }}
          >
            <Input.TextArea rows={1} placeholder="例如：图片生成主通道..." autoSize={{ minRows: 1, maxRows: 3 }} />
          </Form.Item>
          <Form.Item
            name="status"
            label="状态"
            initialValue={1}
            valuePropName="checked"
            getValueFromEvent={(checked: boolean) => (checked ? 1 : 0)}
            getValueProps={(value) => ({ checked: value !== 0 })}
            style={{ marginBottom: 0 }}
          >
            <Switch checkedChildren={t('common.active')} unCheckedChildren={t('common.disabled')} />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title="日额度刷新设置"
        open={dailyResetModalOpen}
        onCancel={() => setDailyResetModalOpen(false)}
        onOk={saveDailyResetModal}
        okText="保存"
        cancelText="取消"
        width={360}
        destroyOnHidden
        getContainer={() => document.body}
        zIndex={1100}
      >
        <Form layout="vertical" size="small" style={{ marginTop: 8 }}>
          <Form.Item
            label={`每天刷新时间点${timedisplayOffsetSuffix(quotaTz)}`}
            style={{ marginBottom: 12 }}
          >
            <TimePicker
              format="HH:mm"
              allowClear={false}
              style={{ width: '100%' }}
              getPopupContainer={(node) => node.parentElement || document.body}
              value={dayjs().hour(dailyResetDraft.hour).minute(dailyResetDraft.minute).second(0)}
              onChange={(t) => {
                setDailyResetDraft((prev) => ({
                  ...prev,
                  hour: t ? t.hour() : 0,
                  minute: t ? t.minute() : 0,
                }));
              }}
            />
          </Form.Item>
          <Form.Item
            label="刷新冷却（分钟）"
            extra="到达时间点后再等待该时长才清零日已用"
            style={{ marginBottom: 8 }}
          >
            <InputNumber
              min={0}
              max={1440}
              placeholder="0"
              style={{ width: '100%' }}
              addonAfter="分钟"
              value={dailyResetDraft.cooldown}
              onChange={(v) => setDailyResetDraft((prev) => ({
                ...prev,
                cooldown: v == null ? 0 : Number(v),
              }))}
            />
          </Form.Item>
          <Text type="secondary" style={{ fontSize: 11 }}>
            按站点默认时区计算（非系统运行时区）；默认 00:00 / 冷却 0 即自然日刷新
          </Text>
        </Form>
      </Modal>

      <ChannelCategoryManager
        visible={isCategoryManagerVisible}
        onClose={() => setIsCategoryManagerVisible(false)}
        onUpdate={fetchCategories}
      />
    </Card>
  );
};

export default ChannelConfigs;
