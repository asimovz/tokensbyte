/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

import React, { useEffect, useState } from 'react';
import { Table, Input, Button, Space, Tag, Typography, App, DatePicker } from 'antd';
import { ReloadOutlined, SearchOutlined } from '@ant-design/icons';
import type { SorterResult } from 'antd/es/table/interface';
import dayjs from 'dayjs';
import request from '../../../utils/request';
import { formatApiDateTime } from '../../../utils/timedisplay';

const { Text } = Typography;

interface HaAttempt {
  n: number;
  yid?: string;
  name?: string;
  status: number;
  error?: string;
  url?: string;
  ms: number;
  ok: number;
}

interface HaLog {
  log_id: number;
  biz_log_id?: string;
  attempt_count: number;
  final_ok: number;
  final_status_code: number;
  attempts: HaAttempt[];
  created_at: string;
  user_uid?: string;
  user_nickname?: string;
  channel_name?: string;
  model?: string;
  status_code?: number;
}

const todayRange = (): [dayjs.Dayjs, dayjs.Dayjs] => [dayjs().startOf('day'), dayjs().endOf('day')];

const HaLogs: React.FC = () => {
  const { message } = App.useApp();
  const [logs, setLogs] = useState<HaLog[]>([]);
  const [loading, setLoading] = useState(false);
  const [total, setTotal] = useState(0);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(15);
  const [keyword, setKeyword] = useState('');
  const [dateRange, setDateRange] = useState<[dayjs.Dayjs | null, dayjs.Dayjs | null]>(todayRange);
  const [sortBy, setSortBy] = useState<string | undefined>(undefined);
  const [sortOrder, setSortOrder] = useState<'asc' | 'desc'>('desc');

  const fetchLogs = async (opt?: { p?: number; size?: number; kw?: string; range?: typeof dateRange; sb?: string; so?: 'asc' | 'desc' }) => {
    const p = opt?.p ?? page;
    const size = opt?.size ?? pageSize;
    const kw = opt?.kw ?? keyword;
    const range = opt?.range ?? dateRange;
    const sb = opt?.sb ?? sortBy;
    const so = opt?.so ?? sortOrder;

    try {
      setLoading(true);
      const res = (await request.get('/plugins/high_availability_channel/ha-logs', {
        params: {
          page: p,
          page_size: size,
          keyword: kw.trim() || undefined,
          date_from: range[0] ? range[0].format('YYYY-MM-DD') : undefined,
          date_to: range[1] ? range[1].format('YYYY-MM-DD') : undefined,
          sort_by: sb,
          sort_order: so,
        },
      })) as { logs?: HaLog[]; total?: number; page?: number; page_size?: number };
      setLogs(res?.logs || []);
      setTotal(res?.total || 0);
      setPage(res?.page || p);
      setPageSize(res?.page_size || size);
    } catch (e: any) {
      message.error(e?.message || '加载失败');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchLogs({ p: 1, kw: '', range: todayRange(), so: 'desc' });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleTableChange = (_: any, __: any, sorter: SorterResult<HaLog> | SorterResult<HaLog>[]) => {
    const s = Array.isArray(sorter) ? sorter[0] : sorter;
    const sb = s.order ? (s.field as string) : undefined;
    const so: 'asc' | 'desc' = s.order === 'ascend' ? 'asc' : 'desc';
    setSortBy(sb);
    setSortOrder(so);
    fetchLogs({ sb, so });
  };

  return (
    <div>
      <Space style={{ marginBottom: 12 }} wrap>
        <Input
          allowClear
          placeholder="日志ID / 用户 / 模型 / 组AID"
          value={keyword}
          onChange={(e) => setKeyword(e.target.value)}
          onPressEnter={() => fetchLogs({ p: 1 })}
          style={{ width: 260 }}
          prefix={<SearchOutlined />}
        />
        <DatePicker.RangePicker
          value={dateRange}
          onChange={(v) => setDateRange(v ? [v[0], v[1]] : [null, null])}
          style={{ width: 240 }}
          placeholder={['开始日期', '结束日期']}
          allowClear
        />
        <Button type="primary" onClick={() => fetchLogs({ p: 1 })}>
          查询
        </Button>
        <Button icon={<ReloadOutlined />} onClick={() => fetchLogs()}>
          刷新
        </Button>
      </Space>
      <Table
        rowKey="log_id"
        loading={loading}
        dataSource={logs}
        size="small"
        onChange={handleTableChange}
        expandable={{
          expandedRowRender: (row) => {
            const attempts = Array.isArray(row.attempts) ? row.attempts : [];
            if (!attempts.length) return <Text type="secondary">无子渠尝试明细</Text>;
            return (
              <div style={{ padding: '4px 0' }}>
                {attempts.map((a) => (
                  <div
                    key={a.n}
                    style={{
                      marginBottom: 10,
                      paddingBottom: 8,
                      borderBottom: '1px solid rgba(0,0,0,0.06)',
                    }}
                  >
                    <Space wrap size={8}>
                      <Tag>#{a.n}</Tag>
                      <Tag color={a.ok ? 'success' : 'error'}>{a.status}</Tag>
                      <Text>{a.name || '-'}</Text>
                      <Text type="secondary">YID {a.yid || '-'}</Text>
                      <Text type="secondary">{a.ms ?? 0} ms</Text>
                    </Space>
                    {a.error ? (
                      <div style={{ color: '#ff4d4f', marginTop: 4, wordBreak: 'break-all' }}>{a.error}</div>
                    ) : null}
                    {a.url ? (
                      <div style={{ marginTop: 4, wordBreak: 'break-all' }}>
                        <Text type="secondary" style={{ fontSize: 12 }}>
                          {a.url}
                        </Text>
                      </div>
                    ) : null}
                  </div>
                ))}
              </div>
            );
          },
        }}
        pagination={{
          current: page,
          pageSize,
          total,
          showSizeChanger: true,
          showQuickJumper: true,
          onChange: (p, s) => fetchLogs({ p, size: s }),
        }}
        columns={[
          {
            title: '时间',
            dataIndex: 'created_at',
            width: 170,
            render: (v: string) => formatApiDateTime(v),
          },
          {
            title: '结果',
            width: 90,
            render: (_: unknown, r: HaLog) =>
              r.final_ok ? (
                <Tag color="success">成功</Tag>
              ) : (
                <Tag color="error">{r.final_status_code || r.status_code || '-'}</Tag>
              ),
          },
          {
            title: '尝试',
            dataIndex: 'attempt_count',
            width: 70,
            sorter: true,
          },
          {
            title: '用户',
            width: 120,
            render: (_: unknown, r: HaLog) => r.user_nickname || r.user_uid || '-',
          },
          { title: '模型', dataIndex: 'model', ellipsis: true },
          { title: '渠道', dataIndex: 'channel_name', ellipsis: true },
          {
            title: '业务日志',
            dataIndex: 'biz_log_id',
            width: 160,
            ellipsis: true,
            render: (v: string) => v || '-',
          },
        ]}
      />
    </div>
  );
};

export default HaLogs;

