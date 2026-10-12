/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

import React, { useEffect, useState, useCallback } from 'react';
import {
  Table, Card, Input, Select, DatePicker, Space, Button, Tag, Drawer,
  Typography, message, Tooltip,
} from 'antd';
import { SearchOutlined, ReloadOutlined, EyeOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import dayjs from 'dayjs';
import request from '../../utils/request';

const { Text, Title } = Typography;
const { RangePicker } = DatePicker;

interface AssetLogRow {
  id: number;
  user_id: string;
  user_display: string | null;
  level_name: string | null;
  channel_name: string | null;
  binding_name: string | null;
  api_endpoint: string;
  status_code: number | null;
  source: string;
  created_at: string;
}

interface AssetLogDetail extends AssetLogRow {
  request_payload: string | null;
  response_payload: string | null;
}

const ACTION_OPTIONS = [
  'CreateAssetGroup', 'ListAssetGroups', 'GetAssetGroup', 'UpdateAssetGroup', 'DeleteAssetGroup',
  'CreateAsset', 'ListAssets', 'GetAsset', 'UpdateAsset', 'DeleteAsset',
];

const SOURCE_OPTIONS = [
  { value: 'all', label: '全部来源' },
  { value: 'upstream_relay_convert', label: '上游透传' },
  { value: 'api_proxy', label: '商业插件' },
  { value: 'relay_convert', label: '素材转换' },
];

const AssetLogs: React.FC = () => {
  const { t } = useTranslation();
  const [data, setData] = useState<AssetLogRow[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(20);

  // 筛选
  const [keyword, setKeyword] = useState('');
  const [level, setLevel] = useState('');
  const [action, setAction] = useState('');
  const [source, setSource] = useState('all');
  const [dateRange, setDateRange] = useState<[string, string] | null>(null);
  const [levels, setLevels] = useState<{ id: number; name: string }[]>([]);
  const [bindings, setBindings] = useState<{ id: number; name: string }[]>([]);
  const [bindingId, setBindingId] = useState<number | undefined>(undefined);

  // 详情 Drawer
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [detail, setDetail] = useState<AssetLogDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);

  // 加载筛选项
  useEffect(() => {
    request.get('/user_levels').then((res: any) => {
      const list = Array.isArray(res) ? res : res?.list || res?.data || [];
      setLevels(list.map((l: any) => ({ id: l.id, name: l.name })));
    }).catch(() => {});
    request.get('/upstream-asset-bindings').then((res: any) => {
      const list = Array.isArray(res) ? res : res?.list || res?.data || [];
      setBindings(list.map((b: any) => ({ id: b.id, name: b.name })));
    }).catch(() => {});
  }, []);

  const fetchData = useCallback(async (p?: number, ps?: number) => {
    setLoading(true);
    try {
      const params: Record<string, any> = {
        page: p ?? page,
        page_size: ps ?? pageSize,
      };
      if (keyword) params.keyword = keyword;
      if (level) params.level = level;
      if (action) params.action = action;
      if (source && source !== 'all') params.source = source;
      else if (source === 'all') params.source = 'all';
      if (bindingId) params.binding_id = bindingId;
      if (dateRange) {
        params.date_from = dateRange[0];
        params.date_to = dateRange[1];
      }
      const res: any = await request.get('/asset-logs', { params });
      setData(res?.list || []);
      setTotal(res?.total || 0);
    } catch (e: any) {
      message.error(e?.message || '加载失败');
    } finally {
      setLoading(false);
    }
  }, [page, pageSize, keyword, level, action, source, bindingId, dateRange]);

  useEffect(() => { fetchData(); }, []);

  const handleSearch = () => {
    setPage(1);
    fetchData(1, pageSize);
  };

  const handleReset = () => {
    setKeyword('');
    setLevel('');
    setAction('');
    setSource('all');
    setBindingId(undefined);
    setDateRange(null);
    setPage(1);
    setTimeout(() => fetchData(1, pageSize, ), 0);
  };

  const openDetail = async (id: number) => {
    setDetailLoading(true);
    setDrawerOpen(true);
    try {
      const res: any = await request.get(`/asset-logs/${id}`);
      setDetail(res);
    } catch (e: any) {
      message.error(e?.message || '加载详情失败');
    } finally {
      setDetailLoading(false);
    }
  };

  const columns = [
    {
      title: '请求用户',
      dataIndex: 'user_display',
      key: 'user_display',
      width: 140,
      ellipsis: true,
    },
    {
      title: '用户等级',
      dataIndex: 'level_name',
      key: 'level_name',
      width: 120,
      render: (v: string | null) => v ? <Tag color="blue">{v}</Tag> : <Text type="secondary">—</Text>,
    },
    {
      title: '渠道名称',
      dataIndex: 'channel_name',
      key: 'channel_name',
      width: 150,
      ellipsis: true,
      render: (v: string | null) => v || <Text type="secondary">—</Text>,
    },
    {
      title: '绑定名称',
      dataIndex: 'binding_name',
      key: 'binding_name',
      width: 140,
      ellipsis: true,
      render: (v: string | null) => v || <Text type="secondary">—</Text>,
    },
    {
      title: '请求类型',
      dataIndex: 'api_endpoint',
      key: 'api_endpoint',
      width: 150,
      render: (v: string) => <Tag>{v}</Tag>,
    },
    {
      title: '状态',
      dataIndex: 'status_code',
      key: 'status_code',
      width: 80,
      render: (v: number | null) => {
        if (!v) return '—';
        const color = v >= 200 && v < 300 ? 'green' : 'red';
        return <Tag color={color}>{v}</Tag>;
      },
    },
    {
      title: '来源',
      dataIndex: 'source',
      key: 'source',
      width: 110,
      render: (v: string) => {
        const map: Record<string, string> = {
          upstream_relay_convert: '透传',
          api_proxy: '商业插件',
          relay_convert: '素材转换',
        };
        return map[v] || v;
      },
    },
    {
      title: '请求时间',
      dataIndex: 'created_at',
      key: 'created_at',
      width: 170,
      render: (v: string) => v ? dayjs(v).format('YYYY-MM-DD HH:mm:ss') : '—',
    },
    {
      title: '操作',
      key: 'action',
      width: 60,
      render: (_: any, record: AssetLogRow) => (
        <Tooltip title="查看详情">
          <Button type="link" size="small" icon={<EyeOutlined />} onClick={() => openDetail(record.id)} />
        </Tooltip>
      ),
    },
  ];

  const tryFormatJson = (s: string | null) => {
    if (!s) return '—';
    try { return JSON.stringify(JSON.parse(s), null, 2); } catch { return s; }
  };

  return (
    <Card bordered={false}>
      <Title level={4} style={{ marginBottom: 16 }}>
        {t('menu.asset_logs', '素材请求日志')}
      </Title>

      {/* 筛选栏 */}
      <Space wrap style={{ marginBottom: 16 }}>
        <Input
          placeholder="用户 ID / 昵称"
          value={keyword}
          onChange={(e) => setKeyword(e.target.value)}
          onPressEnter={handleSearch}
          style={{ width: 160 }}
          allowClear
        />
        <Select
          placeholder="用户等级"
          value={level || undefined}
          onChange={(v) => setLevel(v || '')}
          allowClear
          style={{ width: 140 }}
          options={levels.map((l) => ({ value: l.name, label: l.name }))}
        />
        <Select
          placeholder="请求类型"
          value={action || undefined}
          onChange={(v) => setAction(v || '')}
          allowClear
          style={{ width: 160 }}
          options={ACTION_OPTIONS.map((a) => ({ value: a, label: a }))}
        />
        <Select
          placeholder="绑定"
          value={bindingId}
          onChange={(v) => setBindingId(v)}
          allowClear
          style={{ width: 160 }}
          options={bindings.map((b) => ({ value: b.id, label: b.name }))}
        />
        <Select
          value={source}
          onChange={(v) => setSource(v)}
          style={{ width: 120 }}
          options={SOURCE_OPTIONS}
        />
        <RangePicker
          onChange={(_, dateStrings) => {
            if (dateStrings[0] && dateStrings[1]) {
              setDateRange([dateStrings[0], dateStrings[1]]);
            } else {
              setDateRange(null);
            }
          }}
        />
        <Button type="primary" icon={<SearchOutlined />} onClick={handleSearch}>搜索</Button>
        <Button icon={<ReloadOutlined />} onClick={handleReset}>重置</Button>
      </Space>

      {/* 表格 */}
      <Table
        rowKey="id"
        columns={columns}
        dataSource={data}
        loading={loading}
        size="small"
        scroll={{ x: 1100 }}
        pagination={{
          current: page,
          pageSize,
          total,
          showSizeChanger: true,
          showTotal: (t2) => `共 ${t2} 条`,
          onChange: (p, ps) => {
            setPage(p);
            setPageSize(ps);
            fetchData(p, ps);
          },
        }}
      />

      {/* 详情 Drawer */}
      <Drawer
        title={`日志详情 #${detail?.id ?? ''}`}
        open={drawerOpen}
        onClose={() => { setDrawerOpen(false); setDetail(null); }}
        width={640}
        loading={detailLoading}
      >
        {detail && (
          <Space direction="vertical" style={{ width: '100%' }} size={16}>
            <div>
              <Text strong>用户：</Text> {detail.user_display} ({detail.user_id})
            </div>
            <div>
              <Text strong>等级：</Text> {detail.level_name || '—'}
            </div>
            <div>
              <Text strong>渠道：</Text> {detail.channel_name || '—'}
            </div>
            <div>
              <Text strong>绑定：</Text> {detail.binding_name || '—'}
            </div>
            <div>
              <Text strong>Action：</Text> <Tag>{detail.api_endpoint}</Tag>
            </div>
            <div>
              <Text strong>状态码：</Text> <Tag color={detail.status_code === 200 ? 'green' : 'red'}>{detail.status_code}</Tag>
            </div>
            <div>
              <Text strong>时间：</Text> {dayjs(detail.created_at).format('YYYY-MM-DD HH:mm:ss')}
            </div>
            <div>
              <Text strong>请求体：</Text>
              <pre style={{ background: '#f5f5f5', padding: 12, borderRadius: 6, overflow: 'auto', maxHeight: 260, fontSize: 12 }}>
                {tryFormatJson(detail.request_payload)}
              </pre>
            </div>
            <div>
              <Text strong>响应体：</Text>
              <pre style={{ background: '#f5f5f5', padding: 12, borderRadius: 6, overflow: 'auto', maxHeight: 350, fontSize: 12 }}>
                {tryFormatJson(detail.response_payload)}
              </pre>
            </div>
          </Space>
        )}
      </Drawer>
    </Card>
  );
};

export default AssetLogs;
