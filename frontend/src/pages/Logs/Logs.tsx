/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia 
 * @license        MIT (https://www.tokensbyte.ai/)
 */

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { Table, Tag, Card, Typography, Space, Input, Button, Row, Col, Descriptions, theme, Grid, Tooltip, DatePicker, message, Modal, Spin, Select } from 'antd';
import MobileCardList, { MobileCard, CardRow } from '../../components/MobileCardList';
import { RefreshCw, Search, Download, Image as ImageIcon, MessageSquare, Wrench, LayoutGrid, Copy, Cuboid, ListOrdered, Mic, MoreHorizontal } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import request from '../../utils/request';
import { QueryGuard, isRequestAborted } from '../../utils/queryGuard';
import useSettingsStore from '../../store/settings';
import useAuthStore from '../../store/auth';
import { useThemeStore } from '../../store/theme';
import type { RequestLog } from '../../types';
import dayjs from 'dayjs';
import utc from 'dayjs/plugin/utc';
import { formatApiDateTime } from '../../utils/timedisplay';
import { toDateRangeParams } from '../../utils/dateRangeParams';
import { useLogDetailLoader } from '../../hooks/useLogDetailLoader';
import { parsePluginTagMeta } from '../../utils/pluginTagMeta';
dayjs.extend(utc);

const { RangePicker } = DatePicker;

const { Text } = Typography;
const { useBreakpoint } = Grid;

/** 筛选栏“全部错误”哨兵值（不等于具体状态码，走 error_only 过滤） */
const ALL_ERROR_FILTER_VALUE = 'error';

/** 筛选栏常用状态码（含上游常见 422） */
const STATUS_CODE_FILTER_VALUES = [
  '0', '400', '401', '402', '403', '404', '408', '409', '422', '429', '500', '502', '503', '504',
] as const;

/** 筛选栏错误码 → 请求参数；仅非负整数，否则不传 */
function parseStatusCodeFilter(raw?: string): number | undefined {
  const s = raw?.trim() ?? '';
  return /^\d+$/.test(s) ? Number(s) : undefined;
}

type LogListFilters = {
  modelFilter?: string;
  searchKeyword?: string;
  userFilter?: string;
  statusFilter?: string;
  statusCodeFilter?: string;
  dateRange?: [any, any] | null;
  actionTypeFilter?: string;
  routerEp?: string;
  userGroupFilter?: string;
};

/** 列表/导出共用查询条件（不含 page/per_page） */
function buildLogListParams(f: LogListFilters): Record<string, unknown> {
  const params: Record<string, unknown> = {};
  const model = f.modelFilter?.trim();
  if (model) params.model = model;
  if (f.routerEp) params.router_ep = f.routerEp;
  const keyword = f.searchKeyword?.trim();
  if (keyword) params.search_keyword = keyword;
  const uid = f.userFilter?.trim();
  if (uid) params.user_id = uid;
  if (f.userGroupFilter) params.user_group = f.userGroupFilter;
  if (f.statusFilter) params.status = f.statusFilter;
  if (f.statusCodeFilter === ALL_ERROR_FILTER_VALUE) {
    params.error_only = 1;
  } else {
    const statusCode = parseStatusCodeFilter(f.statusCodeFilter);
    if (statusCode !== undefined) params.status_code = statusCode;
  }
  Object.assign(params, toDateRangeParams(f.dateRange, true));
  if (f.actionTypeFilter && f.actionTypeFilter !== '全部') {
    params.action_type = f.actionTypeFilter;
  }
  return params;
}

/** overrides 显式传入 undefined 时覆盖当前值（勿用 ?? / !== undefined） */
function pickFilter<T>(overrides: object | undefined, key: string, current: T): T {
  if (overrides != null && Object.prototype.hasOwnProperty.call(overrides, key)) {
    return (overrides as Record<string, unknown>)[key] as T;
  }
  return current;
}


const CopyButton: React.FC<{ text: string, color?: string }> = ({ text, color }) => {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);

  return (
    <Tooltip title={copied ? t('logs.copy_success', '已复制') : t('logs.copy', '复制')} open={copied || undefined}>
      <Button 
        size="small" 
        type="text" 
        icon={<Copy size={14} />} 
        onClick={() => {
          navigator.clipboard.writeText(text).then(() => {
            setCopied(true);
            setTimeout(() => setCopied(false), 2000);
          });
        }} 
        style={{ color: color, height: 22, padding: '0 8px' }}
      >
        {t('logs.copy', '复制')}
      </Button>
    </Tooltip>
  );
};

const ShadcnTabs = ({ value, onChange, options, isLight, themeToken }: any) => {
  return (
    <div style={{
      display: 'inline-flex',
      alignItems: 'center',
      justifyContent: 'center',
      borderRadius: '8px',
      background: themeToken?.colorFillAlter || (isLight ? '#fafafa' : '#1d1d1d'),
      padding: '4px',
      height: '32px',
    }}>
      {options.map((opt: any) => {
        const isActive = value === opt.value;
        return (
          <div
            key={opt.value}
            onClick={() => onChange(opt.value)}
            style={{
              display: 'inline-flex',
              alignItems: 'center',
              justifyContent: 'center',
              whiteSpace: 'nowrap',
              borderRadius: '6px',
              padding: '0 16px',
              height: '100%',
              fontSize: '14px',
              fontWeight: 500,
              transition: 'all 0.2s',
              cursor: 'pointer',
              background: isActive ? 'rgb(72, 72, 72)' : 'transparent',
              color: isActive ? '#fff' : (themeToken?.colorTextSecondary || (isLight ? '#71717a' : '#a1a1aa')),
              boxShadow: isActive ? '0 1px 2px 0 rgba(0, 0, 0, 0.05)' : 'none',
              gap: '6px'
            }}
          >
            {opt.icon}
            {opt.label}
          </div>
        );
      })}
    </div>
  );
};


/** 列表用量：轻量计费字段（不依赖 billing_detail 全文） */
function billingUsageMetrics(record: RequestLog) {
  const cacheCreation = record.billing_cache_creation ?? 0;
  const cacheRead = record.billing_cache_read ?? 0;
  const webSearch = record.billing_web_search ?? 0;
  return { cacheCreation, cacheRead, webSearch, isClaude: cacheCreation > 0 || cacheRead > 0 };
}

function prettyJson(raw?: string | null): string | null | undefined {
  if (!raw) return raw;
  try {
    return JSON.stringify(JSON.parse(raw), null, 2);
  } catch {
    return raw;
  }
}

function clientCtColor(ct: string) {
  return ct.includes('multipart') ? 'orange' : 'blue';
}

const Logs: React.FC<{ routerEp?: string }> = ({ routerEp }) => {
  const { t } = useTranslation();
  const { token: themeToken } = theme.useToken();
  const { settings } = useSettingsStore();
  const { themeMode } = useThemeStore();
  const _isLight = themeMode === 'light';
  const currencySymbol = settings?.currency?.currency_symbol || '$';
  const statusCodeOptions = [
    { value: ALL_ERROR_FILTER_VALUE, label: t('logs.status_code_all_error', '全部错误') },
    ...STATUS_CODE_FILTER_VALUES.map((value) => ({
      value,
      label: t(`logs.status_code_${value}`),
    })),
  ];
  const [logs, setLogs] = useState<RequestLog[]>([]);
  const [loading, setLoading] = useState(true);
  const [total, setTotal] = useState(0);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(20);
  const [modelFilter, setModelFilter] = useState('');
  const [searchKeyword, setSearchKeyword] = useState('');
  const [searchParams] = useSearchParams();
  const [userFilter, setUserFilter] = useState<string | undefined>(searchParams.get('user_id') || undefined);
  const [statusFilter, setStatusFilter] = useState<string | undefined>(undefined);
  const [statusCodeFilter, setStatusCodeFilter] = useState<string | undefined>(undefined);
  const [userGroupFilter, setUserGroupFilter] = useState<string | undefined>(undefined);
  const [userLevels, setUserLevels] = useState<any[]>([]);
  const [allowDetails, setAllowDetails] = useState(true);
  const [dateRange, setDateRange] = useState<[any, any] | null>(() => [dayjs().startOf('day'), dayjs().endOf('day')]);
  const [exporting, setExporting] = useState(false);
  const [stats, setStats] = useState<{ total_cost: number; success_count: number; fail_count: number }>({ total_cost: 0, success_count: 0, fail_count: 0 });
  const { user } = useAuthStore();
  const isSuperAdmin = user?.role === 'admin' && !user?.admin_group_id;
  const screens = useBreakpoint();
  const [actionTypeFilter, setActionTypeFilter] = useState<string>(localStorage.getItem('default_log_type') || '视觉');
  const [isSettingsModalVisible, setIsSettingsModalVisible] = useState(false);
  const [tempDefaultType, setTempDefaultType] = useState<string>('视觉');
  const queryGuardRef = useRef(new QueryGuard());
  const skipNextEffectFetchRef = useRef(false);
  const rowIds = useMemo(() => logs.map((l) => l.id), [logs]);
  const {
    detailCache,
    detailLoadingIds,
    expandedRowKeys,
    handleExpand,
    resetDetailCache,
  } = useLogDetailLoader(rowIds);

  useEffect(() => {
    const guard = queryGuardRef.current;
    return () => guard.dispose();
  }, []);

  useEffect(() => {
    if (isSettingsModalVisible) {
      setTempDefaultType(localStorage.getItem('default_log_type') || '视觉');
    }
  }, [isSettingsModalVisible]);

  useEffect(() => {
    if (user?.role !== 'admin') return;
    request.get('/user_levels').then((res: any) => setUserLevels(res.data || [])).catch(console.error);
  }, [user]);

  const buildParams = useCallback(
    () => buildLogListParams({
      modelFilter,
      searchKeyword,
      userFilter,
      statusFilter,
      statusCodeFilter,
      dateRange,
      actionTypeFilter,
      routerEp,
      userGroupFilter,
    }),
    [modelFilter, searchKeyword, userFilter, statusFilter, statusCodeFilter, dateRange, actionTypeFilter, routerEp, userGroupFilter],
  );

  const fetchLogs = useCallback(async (overrides?: {
    page?: number;
    pageSize?: number;
    modelFilter?: string;
    searchKeyword?: string;
    userFilter?: string | undefined;
    statusFilter?: string | undefined;
    statusCodeFilter?: string | undefined;
    dateRange?: [any, any] | null;
    actionTypeFilter?: string;
    userGroupFilter?: string | undefined;
  }) => {
    const signal = queryGuardRef.current.begin();
    setLoading(true);
    try {
      const o = overrides as object | undefined;
      const params: Record<string, unknown> = {
        page: overrides?.page ?? page,
        per_page: overrides?.pageSize ?? pageSize,
        ...buildLogListParams({
          modelFilter: pickFilter(o, 'modelFilter', modelFilter),
          searchKeyword: pickFilter(o, 'searchKeyword', searchKeyword),
          userFilter: pickFilter(o, 'userFilter', userFilter),
          statusFilter: pickFilter(o, 'statusFilter', statusFilter),
          statusCodeFilter: pickFilter(o, 'statusCodeFilter', statusCodeFilter),
          dateRange: pickFilter(o, 'dateRange', dateRange),
          actionTypeFilter: pickFilter(o, 'actionTypeFilter', actionTypeFilter),
          userGroupFilter: pickFilter(o, 'userGroupFilter', userGroupFilter),
          routerEp,
        }),
      };

      const resp = await (request.get('/logs', { params, signal }) as unknown as Promise<{ data: RequestLog[]; total: number; allow_details?: boolean }>);
      if (!queryGuardRef.current.isCurrent(signal)) return;
      setLogs(resp.data);
      setTotal(resp.total);
      resetDetailCache();
      if (resp.allow_details !== undefined) {
        setAllowDetails(resp.allow_details);
      }
      setStats({
        total_cost: (resp as any).total_cost || 0,
        success_count: (resp as any).success_count || 0,
        fail_count: (resp as any).fail_count || 0,
      });
    } catch (e) {
      if (isRequestAborted(e)) return;
      console.error(e);
    } finally {
      if (queryGuardRef.current.isCurrent(signal)) {
        setLoading(false);
      }
    }
  }, [page, pageSize, modelFilter, searchKeyword, userFilter, statusFilter, statusCodeFilter, dateRange, routerEp, actionTypeFilter, userGroupFilter, resetDetailCache]);



  const handleExport = async () => {
    setExporting(true);
    try {
      const params = buildParams();
      const resp = await request.get('/logs/export', {
        params,
        responseType: 'blob',
        skipErrorHandler: true,
      } as any) as any;
      // 如果返回的是 JSON 错误（blob mimetype 为 application/json），解析提示
      if (resp instanceof Blob && resp.type?.includes('json')) {
        const text = await resp.text();
        const json = JSON.parse(text);
        message.error(json.error || t('logs.export_fail', '导出失败'));
        return;
      }
      const blob = resp instanceof Blob ? resp : new Blob([resp], { type: 'text/csv' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `usage_logs_${dayjs().format('YYYYMMDDHHmmss')}.csv`;
      a.click();
      URL.revokeObjectURL(url);
      message.success(t('logs.export_success', '导出成功'));
    } catch (e: any) {
      // 尝试从 axios 错误响应中提取后端消息
      if (e?.response?.data) {
        try {
          const blob = e.response.data;
          const text = blob instanceof Blob ? await blob.text() : JSON.stringify(blob);
          const json = JSON.parse(text);
          message.error(json.error || t('logs.export_fail', '导出失败'));
        } catch { message.error(t('logs.export_fail', '导出失败')); }
      } else {
        message.error(t('logs.export_fail', '导出失败'));
      }
    } finally {
      setExporting(false);
    }
  };

  useEffect(() => {
    if (skipNextEffectFetchRef.current) {
      skipNextEffectFetchRef.current = false;
      return;
    }
    fetchLogs();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [page, pageSize, actionTypeFilter, statusFilter, userGroupFilter]);

  const columns = ([
    {
      title: t('logs.time'),
      dataIndex: 'created_at',
      key: 'created_at',
      width: 170,
      render: (text: string, record: RequestLog) => (
        <Space direction="vertical" size={0}>
          <Text style={{ fontSize: 12 }}>{formatApiDateTime(text)}</Text>
          {record.log_id && (
            <Text 
              type="secondary" 
              style={{ fontSize: 10, fontFamily: 'monospace', maxWidth: 140 }} 
              ellipsis={{ tooltip: record.log_id }}
              copyable={{ text: record.log_id, tooltips: [t('logs.copy', '复制'), t('logs.copy_success', '已复制')] }}
            >
              {record.log_id}
            </Text>
          )}
          {record.task_id && (
            <Text 
              type="secondary" 
              style={{ fontSize: 10, fontFamily: 'monospace', maxWidth: 140, color: '#1677ff' }} 
              ellipsis={{ tooltip: `Task ID: ${record.task_id}` }}
              copyable={{ text: record.task_id, tooltips: [t('logs.copy', '复制'), t('logs.copy_success', '已复制')] }}
            >
              {record.task_id}
            </Text>
          )}
        </Space>
      ),
    },
    user?.role === 'admin' ? {
      title: t('logs.channel_aid', '渠道信息'),
      dataIndex: 'channel_group_aid',
      key: 'channel_group_aid',
      width: 120,
      render: (text: string, record: RequestLog) => (
        <Space size={4} direction="vertical" style={{ alignItems: 'flex-start' }}>
          <Space size={4}>
            <Text type="secondary" style={{ fontSize: 12 }}>{text ? `AID: ${text}` : '-'}</Text>
            {record.is_ha === 1 && <Tag color="blue" style={{ fontSize: 10, margin: 0, padding: '0 4px', lineHeight: '16px' }}>HA</Tag>}
          </Space>
          {record.yid && (
            <Tag color="cyan" style={{ fontSize: 10, margin: 0, padding: '0 4px', lineHeight: '16px' }}>
              {t('logs.sub_channel', '上游')}: {record.yid}
            </Tag>
          )}
        </Space>
      ),
    } : null,
    user?.role === 'admin' ? {
      title: t('logs.user', '用户'),
      key: 'user',
      width: 180,
      render: (_: any, record: RequestLog) => {
        const name = record.user_nickname || record.user_id?.slice(0, 8) || '-';
        const remark = record.user_admin_remark?.trim();
        return (
          <Space direction="vertical" size={0}>
            <Text style={{ fontSize: 12 }}>
              {name}
              {remark ? <Text type="secondary" style={{ fontSize: 12 }}> {remark}</Text> : null}
            </Text>
            {record.user_uid && <Text type="secondary" style={{ fontSize: 10, fontFamily: 'monospace' }}>UID: {record.user_uid}</Text>}
          </Space>
        );
      },
    } : null,
    {
      title: t('logs.token', '令牌'),
      key: 'token_name',
      width: 120,
      render: (_: any, record: RequestLog) => (
        <Space direction="vertical" size={0}>
          <Text style={{ fontSize: 12 }}>{record.token_name || '-'}</Text>
          {record.token_kid && <Text type="secondary" style={{ fontSize: 10, fontFamily: 'monospace' }}>KID: {record.token_kid}</Text>}
        </Space>
      ),
    },

    {
      title: t('logs.status'),
      dataIndex: 'status_code',
      key: 'status_code',
      width: 100,
      filters: [
        { text: t('logs.success', '成功'), value: 'success' },
        { text: t('logs.fail', '失败'), value: 'fail' },
      ],
      filterMultiple: false,
      filteredValue: statusFilter ? [statusFilter] : null,
      render: (code: number) => {
        if (code === 0) {
          return <Tag icon={<RefreshCw size={14} className="anticon-spin" />} color="processing" style={{ display: 'inline-flex', alignItems: 'center' }}>{t('logs.processing', '处理中')}</Tag>;
        }
        return <Tag color={code === 200 ? 'success' : 'error'}>{code || 400}</Tag>;
      },
    },
    {
      title: t('logs.model'),
      dataIndex: 'model',
      key: 'model',
      width: 180,
      ellipsis: true,
      render: (text: string, record: RequestLog) => (
        <Space direction="vertical" size={0}>
          <Text style={{ fontSize: 12 }}>{text}</Text>
          {user?.role === 'admin' && (record.yid || record.billing_pid || record.forward_eid) && (
            <Space size={4}>
              {record.yid && <Text type="secondary" style={{ fontSize: 10, fontFamily: 'monospace' }}>YID:{record.yid}</Text>}
              {record.billing_pid && <Text type="secondary" style={{ fontSize: 10, fontFamily: 'monospace' }}>PID:{record.billing_pid}</Text>}
              {record.forward_eid && <Text type="secondary" style={{ fontSize: 10, fontFamily: 'monospace' }}>EID:{record.forward_eid}</Text>}
            </Space>
          )}
        </Space>
      ),
    },
    {
      title: t('logs.latency'),
      dataIndex: 'latency_ms',
      key: 'latency_ms',
      width: 80,
      render: (val: number) => <Text style={{ fontSize: 12 }}>{(val / 1000).toFixed(3)}s</Text>,
    },
    {
      title: t('logs.type', '类型'),
      dataIndex: 'is_stream',
      key: 'is_stream',
      width: 60,
      render: (stream: number) => <Text type="secondary" style={{ fontSize: 12 }}>{stream === 1 ? t('logs.stream', '流') : t('logs.non_stream', '非流')}</Text>,
    },
    {
      title: t('logs.usage'),
      key: 'usage',
      width: 100,
      render: (_: any, record: RequestLog) => {
        const { cacheCreation, cacheRead, webSearch, isClaude } = billingUsageMetrics(record);
        return (
          <Space direction="vertical" size={0}>
            <Text type="secondary" style={{ fontSize: 11 }}>{t('logs.input', '输入')}: {record.prompt_tokens}</Text>
            <Text type="secondary" style={{ fontSize: 11 }}>{t('logs.output', '输出')}: {record.completion_tokens}</Text>
            {isClaude ? (
              <>
                {cacheCreation > 0 && <Text type="secondary" style={{ fontSize: 11, color: '#faad14' }}>{t('logs.cache_creation', '缓存创建')}: {cacheCreation}</Text>}
                {cacheRead > 0 && <Text type="secondary" style={{ fontSize: 11, color: '#52c41a' }}>{t('logs.cache_read', '缓存读取')}: {cacheRead}</Text>}
              </>
            ) : (
              (record.cached_tokens ?? 0) > 0 && <Text type="secondary" style={{ fontSize: 11, color: '#52c41a' }}>{t('logs.cache_read', '缓存读取')}: {record.cached_tokens}</Text>
            )}
            {webSearch > 0 && <Text type="secondary" style={{ fontSize: 11, color: '#1677ff' }}>联网搜索: {webSearch}次</Text>}
          </Space>
        );
      },
    },
    {
      title: t('logs.cost'),
      dataIndex: 'cost',
      key: 'cost',
      width: 90,
      render: (val: number, record: RequestLog) => (
        <Space direction="vertical" size={0}>
          {val === 0 || record.billing_refunded || record.billing_failed
            ? <Text type="secondary" style={{ fontSize: 12 }}>-</Text>
            : <Text strong style={{ fontSize: 12, color: themeToken.colorError }}>{currencySymbol}{val.toFixed(6)}</Text>
          }
          {record.billing_refunded && (
            <Tag color="orange" style={{ margin: 0, fontSize: 10, lineHeight: '14px', padding: '0 4px' }}>{t('logs.refunded', '已退费')}</Tag>
          )}
        </Space>
      ),
    },
  ].map((c: any) => c ? { ...c, align: 'center' } : null).filter(Boolean)) as any[];

  const expandedRowRender = (record: RequestLog) => {
    const merged = { ...record, ...detailCache[record.id] };
    const loadingDetail = !!detailLoadingIds[record.id] && !detailCache[record.id];
    const reqJson = prettyJson(merged.request_content);
    const respJson = prettyJson(merged.response_content);
    const postRespJson = prettyJson(merged.post_response);
    // 上游出参仅超管展示；用户端 API 已不返回该字段
    const upstreamReqJson = user?.role === 'admin' ? prettyJson(merged.upstream_req_content) : null;

    const costFormula = t('logs.cost_formula_dynamic', '由绑定的计费模板动态结算');
    const { clientCt, cascadeS1TaskId } = parsePluginTagMeta(merged.plugin_tag);

    // 使用 antd theme token 来适配深色/浅色主题
    const panelBg = themeToken.colorBgElevated;
    const codeBg = themeToken.colorFillQuaternary;
    const codeBorder = themeToken.colorBorderSecondary;

    return (
      <div style={{
        padding: 16, background: panelBg, borderRadius: 8,
        maxWidth: screens.xs ? 'calc(100vw - 64px)' : 'calc(100vw - 320px)',
        overflowX: 'auto', boxSizing: 'border-box'
      }}>
        <Descriptions size="small" column={1} styles={{ label: { width: '100px', color: themeToken.colorTextSecondary }, content: { wordBreak: 'break-all', whiteSpace: 'pre-wrap' } }}>
          <Descriptions.Item label={t('logs.system_endpoint', '系统请求路径')}>
            {record.endpoint.startsWith('http') ? record.endpoint : `${window.location.origin}${record.endpoint.startsWith('/') ? '' : '/'}${record.endpoint}`}
          </Descriptions.Item>
          {clientCt && (
            <Descriptions.Item label={t('logs.client_ct', '请求提交类型')}>
              <Tag color={clientCtColor(clientCt)}>{clientCt}</Tag>
            </Descriptions.Item>
          )}
          {isSuperAdmin && cascadeS1TaskId && (
            <Descriptions.Item label={t('logs.cascade_s1_task_id', '上游任务ID')}>
              <Typography.Text
                copyable={{ text: cascadeS1TaskId, tooltips: [t('logs.copy', '复制'), t('logs.copy_success', '已复制')] }}
                style={{ fontSize: 12, fontFamily: 'monospace' }}
              >
                {cascadeS1TaskId}
              </Typography.Text>
            </Descriptions.Item>
          )}
          {record.upstream_url && (
            <Descriptions.Item label={t('logs.upstream_url', '真实上游地址')}>
              {record.upstream_url}
            </Descriptions.Item>
          )}
          {user?.role === 'admin' && (
            <Descriptions.Item label={t('logs.channel_aid', '渠道信息')}>
              <Space size={4}>
                <span>{record.channel_group_aid ? `AID: ${record.channel_group_aid}` : '-'}</span>
                {record.is_ha === 1 && <Tag color="blue" style={{ fontSize: 10, margin: 0, padding: '0 4px', lineHeight: '16px' }}>HA</Tag>}
              </Space>
            </Descriptions.Item>
          )}
          {user?.role === 'admin' && record.sub_channel_name && (
            <Descriptions.Item label={t('logs.sub_channel_name', '实际调用上游')}>
              <Tag color="cyan">{record.sub_channel_name}</Tag>
            </Descriptions.Item>
          )}
          <Descriptions.Item label={t('logs.error_msg', '错误信息')}>
            {(() => {
              if (!record.error_message) {
                if (record.billing_refunded || record.billing_failed) {
                  return <Text type="danger">{t('logs.see_response', '查看下方的响应结果')}</Text>;
                }
                return <Text type="secondary">{t('logs.none', '无')}</Text>;
              }
              try {
                const parsed = JSON.parse(record.error_message);
                return (
                  <div style={{
                    marginTop: 4, maxHeight: 300, overflow: 'auto',
                    background: codeBg, border: `1px solid ${codeBorder}`,
                    padding: 8, borderRadius: 6,
                  }}>
                    <pre style={{ margin: 0, fontSize: 11, whiteSpace: 'pre-wrap', wordBreak: 'break-all', color: themeToken.colorError }}>
                      {JSON.stringify(parsed, null, 2)}
                    </pre>
                  </div>
                );
              } catch (e) {
                return <Text type="danger">{record.error_message}</Text>;
              }
            })()}
          </Descriptions.Item>
          {user?.role === 'admin' && (
          <Descriptions.Item label={t('logs.match_rule', '匹配规则')}>
            <Space size={16} wrap>
              <Text type="secondary" style={{ fontSize: 12 }}>{t('logs.billing_rule', '计费规则 (PID)')}: {record.billing_pid ? <Typography.Text keyboard>{record.billing_pid}</Typography.Text> : '-'}</Text>
              <Text type="secondary" style={{ fontSize: 12 }}>{t('logs.forward_rule', '转发规则 (EID)')}: {record.forward_eid ? <Typography.Text keyboard>{record.forward_eid}</Typography.Text> : '-'}</Text>
              {merged.plugin_tag && (() => {
                try {
                  const tag = JSON.parse(merged.plugin_tag);
                  if (tag && tag.name === 'happyhorse' && (tag.custom_model || tag.actual_model)) {
                    return (
                      <Tag color="purple" style={{ fontSize: 11 }}>
                        {tag.title || tag.name}: {tag.custom_model} → {tag.actual_model} ({tag.media_type})
                      </Tag>
                    );
                  }
                  return null;
                } catch { return null; }
              })()}
            </Space>
          </Descriptions.Item>
          )}
          <Descriptions.Item label={t('logs.billing_detail', '计费明细')}>
            <div>
              {merged.billing_detail ? (
                <Text type="secondary" style={{ fontSize: 12 }}>{t('logs.calc_basis', '计算依据')}: {merged.billing_detail}</Text>
              ) : (
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {loadingDetail ? t('logs.loading_detail', '加载详情中...') : costFormula}
                </Text>
              )}
              <br />
              <Text strong style={{ fontSize: 13, textDecoration: record.billing_refunded ? 'line-through' : 'none' }}>
                {record.billing_refunded ? t('logs.pre_deduct', '预扣费') : t('logs.actual_deduct', '实际扣费')}: {currencySymbol}{record.cost.toFixed(6)}
              </Text>
              {record.billing_refunded && <Text type="danger" style={{ fontSize: 13, marginLeft: 8 }}>{t('logs.fully_refunded', '已全额退回')}</Text>}
              <Text type="secondary" style={{ fontSize: 11, marginLeft: 8 }}>{t('logs.cost_priority', '（优先扣令牌配额，不足扣用户余额）')}</Text>
            </div>
          </Descriptions.Item>
        </Descriptions>

        {loadingDetail ? (
          <div style={{ padding: 32, textAlign: 'center' }}><Spin tip={t('logs.loading_detail', '加载详情中...')} /></div>
        ) : (
        <Row gutter={[16, 16]} style={{ marginTop: 16 }}>
          <Col span={24} style={{ maxWidth: '100%', overflow: 'hidden' }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
              <Text strong style={{ fontSize: 13 }}>{t('logs.req_params', '请求参数（入参）')}</Text>
              {reqJson && <CopyButton text={reqJson} color={themeToken.colorTextSecondary} />}
            </div>
            <div style={{
              marginTop: 8, maxHeight: 300, overflow: 'auto',
              background: codeBg, border: `1px solid ${codeBorder}`,
              padding: 12, borderRadius: 6,
            }}>
              <pre style={{ margin: 0, fontSize: 11, whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
                {reqJson || t('logs.no_context', '该模型未开启「记录上下文」或内容为空')}
              </pre>
            </div>
          </Col>
          {user?.role === 'admin' && (
          <Col span={24} style={{ maxWidth: '100%', overflow: 'hidden' }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
              <Text strong style={{ fontSize: 13 }}>{t('logs.upstream_req_params', '真实转发给上游的请求参数（出参）')}</Text>
              {upstreamReqJson && <CopyButton text={upstreamReqJson} color={themeToken.colorTextSecondary} />}
            </div>
            <div style={{
              marginTop: 8, maxHeight: 300, overflow: 'auto',
              background: codeBg, border: `1px solid ${codeBorder}`,
              padding: 12, borderRadius: 6,
            }}>
              <pre style={{ margin: 0, fontSize: 11, whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
                {upstreamReqJson || t('logs.same_as_req', '同【请求参数】或未记录。')}
              </pre>
            </div>
          </Col>
          )}
          <Col span={24} style={{ maxWidth: '100%', overflow: 'hidden' }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
              <Text strong style={{ fontSize: 13 }}>{t('logs.resp_content', '响应结果')}</Text>
              {respJson && <CopyButton text={respJson} color={themeToken.colorTextSecondary} />}
            </div>
            <div style={{
              marginTop: 8, maxHeight: 300, overflow: 'auto',
              background: codeBg, border: `1px solid ${codeBorder}`,
              padding: 12, borderRadius: 6,
            }}>
              <pre style={{ margin: 0, fontSize: 11, whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
                {respJson || t('logs.no_context', '该模型未开启「记录上下文」或内容为空')}
              </pre>
            </div>
          </Col>
          {postRespJson && (
            <Col span={24} style={{ maxWidth: '100%', overflow: 'hidden' }}>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                <Text strong style={{ fontSize: 13 }}>{t('logs.post_resp_content', 'POST 提交响应结果')}</Text>
                <CopyButton text={postRespJson} color={themeToken.colorTextSecondary} />
              </div>
              <div style={{
                marginTop: 8, maxHeight: 300, overflow: 'auto',
                background: codeBg, border: `1px solid ${codeBorder}`,
                padding: 12, borderRadius: 6,
              }}>
                <pre style={{ margin: 0, fontSize: 11, whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
                  {postRespJson}
                </pre>
              </div>
            </Col>
          )}
        </Row>
        )}
      </div>
    );
  };

  return (
    <Card 
      variant="borderless" 
      style={{ 
        background: screens.xs ? 'transparent' : (_isLight ? '#fff' : 'rgba(255,255,255,0.02)'), 
        borderRadius: 12,
        boxShadow: screens.xs ? 'none' : undefined
      }} 
      styles={{ body: { padding: screens.xs ? 0 : '16px 24px 24px' } }}
    >
      <div style={{ marginBottom: 16 }}>
        <div style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 12 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 16, flexWrap: 'wrap' }}>
            <Typography.Title level={4} style={{ margin: 0, fontSize: 18, display: 'flex', alignItems: 'center', gap: '8px' }}>
              <RefreshCw size={20} />
              {t('menu.usage_logs', '使用日志')}
            </Typography.Title>
            <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
              <ShadcnTabs 
                value={actionTypeFilter}
                isLight={_isLight}
                themeToken={themeToken}
                onChange={(v: string) => {
                  setActionTypeFilter(v);
                  setPage(1);
                }}
                options={[
                  { value: '视觉', label: t('logs.type_vision', '视觉'), icon: <ImageIcon size={14} /> },
                  { value: '聊天', label: t('logs.type_chat', '聊天'), icon: <MessageSquare size={14} /> },
                  { value: '音频', label: t('logs.type_audio', '音频'), icon: <Mic size={14} /> },
                  { value: '向量', label: t('logs.type_embedding', '向量'), icon: <Cuboid size={14} /> },
                  { value: '排序', label: t('logs.type_rerank', '排序'), icon: <ListOrdered size={14} /> },
                  { value: '其它', label: t('logs.type_other', '其它'), icon: <Wrench size={14} /> },
                  { value: '全部', label: t('logs.type_all', '全部'), icon: <LayoutGrid size={14} /> },
                ]}
              />
              <Tooltip title={t('logs.configure_default_type', '配置默认视图')}>
                <Button
                  type="text"
                  icon={<MoreHorizontal size={16} />}
                  onClick={() => setIsSettingsModalVisible(true)}
                  style={{
                    display: 'inline-flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    borderRadius: '8px',
                    width: '32px',
                    height: '32px',
                    padding: 0,
                    background: themeToken?.colorFillAlter || (_isLight ? '#fafafa' : '#1d1d1d'),
                    border: 'none',
                    color: themeToken?.colorTextSecondary || (_isLight ? '#71717a' : '#a1a1aa'),
                  }}
                />
              </Tooltip>
            </div>
          </div>
        </div>
        {total > 0 && (
          <div style={{ marginTop: 8 }}>
            <Space size={12} wrap split={<span style={{ color: themeToken.colorBorder }}>|</span>}>
              <Text type="secondary" style={{ fontSize: 12 }}>{t('logs.requests', '请求')} <strong style={{ color: themeToken.colorText }}>{total}</strong></Text>
              <Text type="secondary" style={{ fontSize: 12 }}>{t('logs.success', '成功')} <strong style={{ color: themeToken.colorText }}>{stats.success_count}</strong></Text>
              <Text type="secondary" style={{ fontSize: 12 }}>{t('logs.fail', '失败')} <strong style={{ color: themeToken.colorText }}>{stats.fail_count}</strong></Text>
              <Text type="secondary" style={{ fontSize: 12 }}>{t('logs.success_rate', '成功率')} <strong style={{ color: themeToken.colorText }}>{total > 0 ? ((stats.success_count / total) * 100).toFixed(1) : '0.0'}%</strong></Text>
              <Text type="secondary" style={{ fontSize: 12 }}>{t('logs.total_cost', '成本合计')} <strong style={{ color: themeToken.colorText }}>{currencySymbol}{stats.total_cost.toFixed(6)}</strong></Text>
            </Space>
          </div>
        )}
      </div>

      <div style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 8, marginBottom: 16 }}>
          {user?.role === 'admin' && (
            <>
              <Input
                placeholder={t('logs.search_user_id', '搜索用户 UID/用户名')}
                prefix={<Search size={16} />}
                value={userFilter || ''}
                onChange={e => setUserFilter(e.target.value || undefined)}
                onPressEnter={() => fetchLogs()}
                style={{ width: screens.xs ? '100%' : 180, fontSize: 12, height: 32 }}
                allowClear
              />
              <Select
                placeholder={t('logs.search_user_group', '用户等级')}
                value={userGroupFilter}
                onChange={(v) => setUserGroupFilter(v ?? undefined)}
                options={userLevels.map((l: any) => ({ value: l.group_key, label: `${l.name} (${l.discount}x)` }))}
                style={{ width: screens.xs ? '100%' : 180, fontSize: 12, height: 32 }}
                popupMatchSelectWidth={false}
                allowClear
                showSearch
                optionFilterProp="label"
              />
            </>
          )}
          {user?.role !== 'admin' && userFilter && (
            <Input
              value={userFilter}
              style={{ width: screens.xs ? '100%' : 120, fontSize: 12, height: 32 }}
              disabled
            />
          )}
          <Input
            placeholder={t('logs.search_keyword', '搜索日志 ID / 任务 ID / 渠道 AID')}
            prefix={<Search size={16} />}
            value={searchKeyword}
            onChange={e => setSearchKeyword(e.target.value)}
            onPressEnter={() => fetchLogs()}
            style={{ width: screens.xs ? '100%' : 260, fontSize: 12, height: 32 }}
            allowClear
          />
          <Input
            placeholder={t('logs.model_name', '模型名称')}
            prefix={<Search size={16} />}
            value={modelFilter}
            onChange={e => setModelFilter(e.target.value)}
            onPressEnter={() => fetchLogs()}
            style={{ width: screens.xs ? '100%' : 140, fontSize: 12, height: 32 }}
          />
          <Select
            placeholder={t('logs.search_status_code', '错误码')}
            value={statusCodeFilter}
            onChange={(v) => setStatusCodeFilter(v ?? undefined)}
            options={statusCodeOptions}
            style={{ width: screens.xs ? '100%' : 160, fontSize: 12, height: 32 }}
            allowClear
            showSearch
            optionFilterProp="label"
          />
          <RangePicker
            value={dateRange}
            placeholder={[t('logs.start_date', '开始时间'), t('logs.end_date', '结束时间')]}
            onChange={(vals) => setDateRange(vals as [any, any] | null)}
            showTime={{ format: 'HH:mm' }}
            format="YYYY-MM-DD HH:mm"
            className="font-size-12"
            style={{ width: screens.xs ? '100%' : 380, fontSize: 12, height: 32 }}
          />
          <Space size={8} style={{ marginLeft: screens.xs ? 0 : 'auto' }}>
            <Button type="primary" icon={<Search size={14} />} onClick={() => fetchLogs()} loading={loading} disabled={loading} style={{ height: 32, borderRadius: 6, fontSize: 12, display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }}>{t('logs.query', '查询')}</Button>
            <Button icon={<RefreshCw size={14} />} onClick={() => fetchLogs()} loading={loading} disabled={loading} style={{ height: 32, borderRadius: 6, fontSize: 12, display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }}>{t('common.refresh', '刷新')}</Button>
            <Tooltip title={user?.role === 'admin'
              ? t('logs.export_tooltip', '根据当前筛选条件导出 CSV（上限10万条）')
              : t('logs.export_tooltip_self', '导出我自己的日志 CSV（上限10万条）')}>
              <Button icon={<Download size={14} />} loading={exporting} disabled={loading} onClick={handleExport} style={{ height: 32, borderRadius: 6, fontSize: 12, display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }}>{t('logs.export', '导出')}</Button>
            </Tooltip>
          </Space>
        </div>

      {screens.xs ? (
        <MobileCardList
          dataSource={logs}
          loading={loading}
          rowKey="id"
          compact={true}
          gap={4}
          pagination={{
            total,
            current: page,
            pageSize,
            onChange: (p: number, s: number) => { setPage(p); setPageSize(s); },
            showQuickJumper: true,
          }}
          renderCard={(record: any) => {
            const userName = record.user_nickname || record.user_id?.slice(0, 8) || '-';
            return (
              <MobileCard
                compact={true}
                style={{ background: _isLight ? '#fff' : '#141414', border: 'none' }}
                title={<Tag color="blue">{record.model}</Tag>}
                extra={record.status_code === 0
                  ? <Tag icon={<RefreshCw size={14} className="anticon-spin" />} color="processing" style={{ display: 'inline-flex', alignItems: 'center' }}>{t('logs.processing', '处理中')}</Tag>
                  : <Tag color={record.status_code === 200 ? 'success' : 'error'}>{record.status_code || 400}</Tag>
                }
              >
                <CardRow label={t('logs.time', '时间')}>
                  <Space direction="vertical" size={0} align="end">
                    <Text type="secondary" style={{ fontSize: 12 }}>{formatApiDateTime(record.created_at)}</Text>
                    {record.log_id && (
                      <Text 
                        type="secondary" 
                        style={{ fontSize: 10, fontFamily: 'monospace', maxWidth: '45vw' }} 
                        ellipsis={{ tooltip: record.log_id }}
                        copyable={{ text: record.log_id }}
                      >
                        {record.log_id}
                      </Text>
                    )}
                  </Space>
                </CardRow>
                {user?.role === 'admin' && (
                  <CardRow label={t('logs.user', '用户')}>
                    <Space direction="vertical" size={0}>
                      <Text style={{ fontSize: 12 }}>
                        {userName}
                        {record.user_admin_remark?.trim() ? <Text type="secondary" style={{ fontSize: 12 }}> {record.user_admin_remark.trim()}</Text> : null}
                      </Text>
                      {record.user_uid && <Text type="secondary" style={{ fontSize: 10, fontFamily: 'monospace' }}>UID: {record.user_uid}</Text>}
                    </Space>
                  </CardRow>
                )}
                {user?.role === 'admin' && record.channel_group_aid && <CardRow label={t('logs.channel_aid', '渠道信息')}>
                  <Space size={4}>
                    <Text type="secondary" style={{ fontSize: 12 }}>AID: {record.channel_group_aid}</Text>
                    {record.is_ha === 1 && <Tag color="blue" style={{ fontSize: 10, margin: 0, padding: '0 4px', lineHeight: '16px' }}>HA</Tag>}
                  </Space>
                </CardRow>}
                {user?.role === 'admin' && record.yid && <CardRow label={t('logs.sub_channel_name', '实际调用上游')}>
                  <Tag color="cyan" style={{ fontSize: 11 }}>{record.yid}</Tag>
                </CardRow>}
                {user?.role === 'admin' && (record.yid || record.billing_pid || record.forward_eid) && (
                  <CardRow label={t('logs.match_rule', '匹配规则')}>
                    <Space size={8}>
                      {record.yid && <Text type="secondary" style={{ fontSize: 11 }}>YID:<Typography.Text keyboard style={{ fontSize: 10 }}>{record.yid}</Typography.Text></Text>}
                      {record.billing_pid && <Text type="secondary" style={{ fontSize: 11 }}>PID:<Typography.Text keyboard style={{ fontSize: 10 }}>{record.billing_pid}</Typography.Text></Text>}
                      {record.forward_eid && <Text type="secondary" style={{ fontSize: 11 }}>EID:<Typography.Text keyboard style={{ fontSize: 10 }}>{record.forward_eid}</Typography.Text></Text>}
                    </Space>
                  </CardRow>
                )}
                <CardRow label={t('logs.token', '令牌')}>
                  <Space size={6} align="center" style={{ justifyContent: 'flex-end', width: '100%' }}>
                    <Tag color="cyan" style={{ fontSize: 11, margin: 0 }}>{record.token_name || '-'}</Tag>
                    {record.token_kid && <Text type="secondary" style={{ fontSize: 10, fontFamily: 'monospace' }}>KID: {record.token_kid}</Text>}
                  </Space>
                </CardRow>
                <CardRow label={t('logs.latency', '耗时')}><Text style={{ fontSize: 12 }}>{(record.latency_ms / 1000).toFixed(3)}s</Text></CardRow>
                <CardRow label={t('logs.type', '类型')}><Tag color={record.is_stream === 1 ? 'geekblue' : 'default'}>{record.is_stream === 1 ? t('logs.stream', '流') : t('logs.non_stream', '非流')}</Tag></CardRow>
                <CardRow label={t('logs.usage', '用量')}>
                  <Space direction="vertical" size={0} align="end">
                    <Text type="secondary" style={{ fontSize: 12 }}>输入:{record.prompt_tokens} / 输出:{record.completion_tokens}</Text>
                    {(() => {
                      const { cacheCreation: cc, cacheRead: cr } = billingUsageMetrics(record);
                      if (cc > 0 || cr > 0) {
                        return (
                          <>
                            {cc > 0 && <Text type="secondary" style={{ fontSize: 11, color: '#faad14' }}>缓存创建:{cc}</Text>}
                            {cr > 0 && <Text type="secondary" style={{ fontSize: 11, color: '#52c41a' }}>缓存读取:{cr}</Text>}
                          </>
                        );
                      }
                      return (record.cached_tokens ?? 0) > 0 ? <Text type="secondary" style={{ fontSize: 11, color: '#52c41a' }}>{t('logs.cache_read', '缓存读取')}:{record.cached_tokens}</Text> : null;
                    })()}
                  </Space>
                </CardRow>
                <CardRow label={t('logs.cost', '成本')}>
                  <Space direction="vertical" size={0} align="end">
                    {record.cost === 0 || record.billing_refunded || record.billing_failed
                      ? <Text type="secondary" style={{ fontSize: 12 }}>-</Text>
                      : <Text strong style={{ fontSize: 12, color: themeToken.colorError }}>{currencySymbol}{record.cost.toFixed(6)}</Text>
                    }
                    {record.billing_refunded && (
                      <Tag color="orange" style={{ margin: 0, fontSize: 10, lineHeight: '14px', padding: '0 4px' }}>{t('logs.refunded', '已退费')}</Tag>
                    )}
                  </Space>
                </CardRow>
              </MobileCard>
            );
          }}
        />
      ) : (
        <Table
          dataSource={logs}
          columns={columns}
          rowKey="id"
          loading={loading}
          expandable={
            allowDetails ? {
              expandedRowKeys,
              expandedRowRender,
              onExpand: handleExpand,
              expandRowByClick: false
            } : undefined
          }
          pagination={{
            total,
            current: page,
            pageSize,
            onChange: (p, s) => { setPage(p); setPageSize(s); },
            showSizeChanger: true,
            showQuickJumper: true,
          }}
          onChange={(pagination, filters: any) => {
            let shouldResetPage = false;
            
            const stFilter = filters.status_code ? filters.status_code[0] as string : undefined;
            if (stFilter !== statusFilter) {
              setStatusFilter(stFilter);
              shouldResetPage = true;
            }

            if (shouldResetPage) {
              setPage(1);
            }
          }}
          size="middle"
          locale={{ emptyText: t('dashboard.no_data') }}
          scroll={{ x: 'max-content' }}
        />
      )}

      <Modal
        title={null}
        open={isSettingsModalVisible}
        onCancel={() => setIsSettingsModalVisible(false)}
        footer={null}
        width={420}
        styles={{
          mask: { backgroundColor: 'rgba(0, 0, 0, 0.45)' },
          body: { padding: '24px' },
          content: { 
            backgroundColor: _isLight ? 'rgba(255, 255, 255, 0.95)' : 'rgba(28, 29, 31, 0.85)', 
            backdropFilter: 'blur(16px) saturate(120%)', 
            WebkitBackdropFilter: 'blur(16px) saturate(120%)',
            borderRadius: 16, 
            padding: 0, 
            overflow: 'hidden',
            border: _isLight ? '1px solid rgba(0,0,0,0.1)' : '1px solid rgba(255,255,255,0.1)',
            boxShadow: _isLight ? '0 12px 32px rgba(0,0,0,0.1)' : '0 24px 48px rgba(0,0,0,0.4)'
          },
        } as any}
      >
        <div style={{ marginBottom: 20 }}>
          <h3 style={{ fontSize: 16, fontWeight: 600, margin: 0, color: _isLight ? '#1f2937' : '#E8EAED' }}>
            {t('logs.default_type_modal_title', '配置默认查看日志类型')}
          </h3>
          <p style={{ fontSize: 12, color: _isLight ? '#71717a' : '#a1a1aa', margin: '4px 0 0 0' }}>
            {t('logs.default_type_modal_desc', '选择在进入日志查询页面时，默认加载并展示的日志类别。')}
          </p>
        </div>

        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, 1fr)', gap: 10, marginBottom: 24 }}>
          {[
            { value: '视觉', label: t('logs.type_vision', '视觉'), icon: <ImageIcon size={14} /> },
            { value: '聊天', label: t('logs.type_chat', '聊天'), icon: <MessageSquare size={14} /> },
            { value: '音频', label: t('logs.type_audio', '音频'), icon: <Mic size={14} /> },
            { value: '向量', label: t('logs.type_embedding', '向量'), icon: <Cuboid size={14} /> },
            { value: '排序', label: t('logs.type_rerank', '排序'), icon: <ListOrdered size={14} /> },
            { value: '其它', label: t('logs.type_other', '其它'), icon: <Wrench size={14} /> },
            { value: '全部', label: t('logs.type_all', '全部'), icon: <LayoutGrid size={14} /> },
          ].map((opt) => {
            const isSelected = tempDefaultType === opt.value;
            return (
              <div
                key={opt.value}
                onClick={() => setTempDefaultType(opt.value)}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 8,
                  padding: '10px 12px',
                  borderRadius: 8,
                  border: `1px solid ${isSelected ? themeToken.colorPrimary : (_isLight ? 'rgba(0,0,0,0.08)' : 'rgba(255,255,255,0.08)')}`,
                  background: isSelected 
                    ? (_isLight ? 'rgba(22, 119, 255, 0.05)' : 'rgba(22, 119, 255, 0.15)') 
                    : (_isLight ? 'transparent' : 'rgba(255,255,255,0.02)'),
                  cursor: 'pointer',
                  transition: 'all 0.2s',
                  color: isSelected ? themeToken.colorPrimary : (_isLight ? '#1f2937' : '#E8EAED'),
                }}
                onMouseEnter={(e) => {
                  if (!isSelected) {
                    e.currentTarget.style.borderColor = _isLight ? 'rgba(0,0,0,0.15)' : 'rgba(255,255,255,0.15)';
                    e.currentTarget.style.background = _isLight ? 'rgba(0,0,0,0.01)' : 'rgba(255,255,255,0.04)';
                  }
                }}
                onMouseLeave={(e) => {
                  if (!isSelected) {
                    e.currentTarget.style.borderColor = _isLight ? 'rgba(0,0,0,0.08)' : 'rgba(255,255,255,0.08)';
                    e.currentTarget.style.background = _isLight ? 'transparent' : 'rgba(255,255,255,0.02)';
                  }
                }}
              >
                <span>{opt.icon}</span>
                <span style={{ fontSize: 13, fontWeight: isSelected ? 500 : 400 }}>{opt.label}</span>
              </div>
            );
          })}
        </div>

        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 12 }}>
          <Button
            onClick={() => setIsSettingsModalVisible(false)}
            style={{
              borderRadius: 6,
              height: 32,
              padding: '0 16px',
              fontSize: 13,
              border: _isLight ? '1px solid rgba(0,0,0,0.15)' : '1px solid rgba(255,255,255,0.15)',
              background: 'transparent',
              color: _isLight ? '#1f2937' : '#E8EAED',
            }}
          >
            {t('common.cancel', '取消')}
          </Button>
          <Button
            onClick={() => {
              localStorage.setItem('default_log_type', tempDefaultType);
              setActionTypeFilter(tempDefaultType);
              setPage(1);
              message.success(t('logs.default_type_saved', '默认日志类型配置已保存'));
              setIsSettingsModalVisible(false);
            }}
            style={{
              borderRadius: 6,
              height: 32,
              padding: '0 16px',
              fontSize: 13,
              background: themeToken.colorPrimary,
              color: '#fff',
              border: 'none',
            }}
          >
            {t('common.confirm', '确认')}
          </Button>
        </div>
      </Modal>
    </Card>
  );
};

export default Logs;
