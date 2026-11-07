/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { Alert, Button, Drawer, Empty, Input, Segmented, Spin, Table, Tag, Tooltip, Typography } from 'antd';
import { ReloadOutlined } from '@ant-design/icons';
import request from '../utils/request';

const { Text } = Typography;

type MatchType = 'level' | 'open';

interface SubChannelView {
  config_id: string;
  name: string;
  priority: number;
  weight: number;
  priority_from: 'group' | 'upstream';
  weight_from: 'group' | 'upstream';
}

interface ChannelView {
  id: string;
  name: string;
  is_ha_group: boolean;
  match: MatchType;
  sub_channels: SubChannelView[];
}

interface ModelRow {
  mid: string;
  model_id: string;
  match: MatchType;
  discount: number | null;
  channel_count: number;
  channels: ChannelView[];
}

interface UserModelsResponse {
  user: {
    id: string;
    uid: string;
    username: string;
    nickname: string;
    level_id: string;
    level_name: string | null;
    user_group: string;
  };
  summary: {
    total_active_models: number;
    accessible: number;
    level_only: number;
    open: number;
    discounted: number;
    no_channel: number;
  };
  data: ModelRow[];
  unreachable_discounts: Array<{ mid: string; model_id: string; discount: number | null }>;
}

interface Props {
  open: boolean;
  userId: string | null;
  userName?: string;
  onClose: () => void;
}

const LEVEL_COLOR = '#fa8c16';
const OPEN_COLOR = '#8c8c8c';
const HA_COLOR = '#722ed1';

const statItem = (value: number, label: string, color?: string) => (
  <div style={{ marginRight: 28 }}>
    <div style={{ fontSize: 18, fontWeight: 600, color: color || '#1677ff', lineHeight: '22px' }}>{value}</div>
    <div style={{ fontSize: 12, color: '#8a9099' }}>{label}</div>
  </div>
);

const ChannelCell: React.FC<{ channel: ChannelView; expanded: boolean; onToggle: () => void }> = ({
  channel,
  expanded,
  onToggle,
}) => {
  if (!channel.is_ha_group) {
    return (
      <div style={{ marginBottom: 6 }}>
        <Tag color={channel.match === 'level' ? LEVEL_COLOR : OPEN_COLOR} style={{ marginRight: 0 }}>
          {channel.name}
        </Tag>
        <Text type="secondary" style={{ fontSize: 12, marginLeft: 6 }}>#{channel.id}</Text>
      </div>
    );
  }
  return (
    <div style={{ marginBottom: 6 }}>
      <Tag color={HA_COLOR} style={{ cursor: 'pointer', marginRight: 0 }} onClick={onToggle}>
        {channel.name} {expanded ? '收起' : `组内 ${channel.sub_channels.length} 渠道`}
      </Tag>
      <Text type="secondary" style={{ fontSize: 12, marginLeft: 6 }}>#{channel.id} 渠道分组</Text>
      {expanded && (
        <Table
          size="small"
          pagination={false}
          rowKey="config_id"
          style={{ marginTop: 6, background: '#fafafa' }}
          dataSource={channel.sub_channels}
          columns={[
            { title: '组内渠道', dataIndex: 'name', key: 'name' },
            {
              title: '优先级',
              dataIndex: 'priority',
              key: 'priority',
              width: 120,
              render: (v: number, r: SubChannelView) => (
                <span>
                  {v}
                  <Text type="secondary" style={{ fontSize: 11, marginLeft: 4 }}>
                    {r.priority_from === 'group' ? '组内' : '上游全局'}
                  </Text>
                </span>
              ),
            },
            {
              title: '权重',
              dataIndex: 'weight',
              key: 'weight',
              width: 120,
              render: (v: number, r: SubChannelView) => (
                <span>
                  {v}
                  <Text type="secondary" style={{ fontSize: 11, marginLeft: 4 }}>
                    {r.weight_from === 'group' ? '组内' : '上游全局'}
                  </Text>
                </span>
              ),
            },
          ]}
        />
      )}
    </div>
  );
};

const UserModelsDrawer: React.FC<Props> = ({ open, userId, userName, onClose }) => {
  const [loading, setLoading] = useState(false);
  const [resp, setResp] = useState<UserModelsResponse | null>(null);
  const [seg, setSeg] = useState<MatchType>('level');
  const [keyword, setKeyword] = useState('');
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});

  const load = useCallback(async () => {
    if (!userId) return;
    setLoading(true);
    try {
      const res = (await request.get(`/users/${userId}/models`)) as unknown as UserModelsResponse;
      setResp(res);
    } finally {
      setLoading(false);
    }
  }, [userId]);

  useEffect(() => {
    if (open && userId) {
      setSeg('level');
      setKeyword('');
      setExpanded({});
      load();
    }
  }, [open, userId, load]);

  const rows = useMemo(() => {
    if (!resp) return [];
    const kw = keyword.trim().toLowerCase();
    return resp.data
      .filter((r) => r.match === seg)
      .filter(
        (r) =>
          !kw ||
          r.model_id.toLowerCase().includes(kw) ||
          r.mid.includes(kw) ||
          r.channels.some((c) => c.name.toLowerCase().includes(kw)),
      );
  }, [resp, seg, keyword]);

  const summary = resp?.summary;

  return (
    <Drawer
      title={
        <span>
          可访问模型{' '}
          <Tag color="#1677ff">{resp?.user.level_name || '无等级'}</Tag>
          {userName && <Text type="secondary" style={{ fontSize: 13, marginLeft: 8 }}>{userName}</Text>}
        </span>
      }
      width={920}
      open={open}
      onClose={onClose}
      extra={<Button size="small" icon={<ReloadOutlined />} onClick={load} loading={loading}>刷新</Button>}
    >
      <Spin spinning={loading}>
        {summary && (
          <div style={{ display: 'flex', marginBottom: 14 }}>
            {statItem(summary.accessible, '可访问模型')}
            {statItem(summary.level_only, '本等级专属', LEVEL_COLOR)}
            {statItem(summary.open, '无需等级', OPEN_COLOR)}
            {statItem(summary.discounted, '配了用户模型折扣', '#389e0d')}
            <Tooltip title="启用模型中，该等级没有任何启用渠道挂着它的数量，这些模型不在列表里">
              <span>{statItem(summary.no_channel, '无渠道可挂', '#c7ccd2')}</span>
            </Tooltip>
          </div>
        )}
        {resp && resp.unreachable_discounts.length > 0 && (
          <Alert
            type="warning"
            showIcon
            style={{ marginBottom: 12 }}
            message="以下模型已配用户模型折扣，但该等级没有启用渠道挂着它们（配了也用不上）"
            description={resp.unreachable_discounts
              .map((d) => `${d.model_id || d.mid} ${d.discount ?? ''}`)
              .join('、')}
          />
        )}
        <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 12 }}>
          <Segmented
            value={seg}
            onChange={(v) => setSeg(v as MatchType)}
            options={[
              { value: 'level', label: `本等级专属 ${summary?.level_only ?? 0}` },
              { value: 'open', label: `无需等级 ${summary?.open ?? 0}` },
            ]}
          />
          <Input
            allowClear
            placeholder="搜索模型 / 渠道 / 分组"
            style={{ width: 260 }}
            value={keyword}
            onChange={(e) => setKeyword(e.target.value)}
          />
        </div>
        <Table
          size="small"
          rowKey="mid"
          dataSource={rows}
          locale={{
            emptyText: <Empty description={summary?.accessible === 0 ? '该等级没有渠道可路由的模型' : '没有符合条件的模型'} image={Empty.PRESENTED_IMAGE_SIMPLE} />,
          }}
          pagination={{ pageSize: 50, showSizeChanger: false, size: 'small' }}
          columns={[
            {
              title: '模型',
              key: 'model',
              width: 250,
              render: (_: unknown, r: ModelRow) => (
                <div>
                  <Text style={{ fontFamily: 'ui-monospace, Menlo, monospace', fontSize: 13 }}>{r.model_id}</Text>
                  <div>
                    <Text type="secondary" style={{ fontSize: 11 }}>mid {r.mid}</Text>
                  </div>
                </div>
              ),
            },
            {
              title: '用户模型折扣',
              key: 'discount',
              width: 120,
              align: 'right',
              render: (_: unknown, r: ModelRow) =>
                r.discount === null || r.discount === undefined ? (
                  <Text type="secondary">—</Text>
                ) : (
                  <Tag color="#389e0d">{r.discount}</Tag>
                ),
            },
            {
              title: '渠道 / 渠道分组',
              key: 'channels',
              render: (_: unknown, r: ModelRow) => (
                <div>
                  {r.channels.map((c) => (
                    <ChannelCell
                      key={`${r.mid}-${c.id}`}
                      channel={c}
                      expanded={!!expanded[`${r.mid}-${c.id}`]}
                      onToggle={() => setExpanded((prev) => ({ ...prev, [`${r.mid}-${c.id}`]: !prev[`${r.mid}-${c.id}`] }))}
                    />
                  ))}
                </div>
              ),
            },
          ]}
        />
        <div style={{ marginTop: 12, fontSize: 12, color: '#8a9099', lineHeight: '20px' }}>
          <div>
            <Text strong style={{ fontSize: 12, color: '#595959' }}>「可访问」= 该用户等级至少有一条启用渠道挂着这个模型</Text>
            ，只看渠道配置，不代表调用一定成功（渠道额度、熔断、上游预设状态不在本视图内）。
          </div>
          <div>
            紫色为 <Tag color={HA_COLOR} style={{ fontSize: 11 }}>渠道分组</Tag>，点标签展开组内渠道；组内渠道本身不带模型清单，成员只决定往哪个上游转发。
            折扣列只取「用户模型折扣」一项，等级折扣与全站折扣保持默认。
          </div>
        </div>
      </Spin>
    </Drawer>
  );
};

export default UserModelsDrawer;
