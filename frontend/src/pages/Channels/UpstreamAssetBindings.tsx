/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

import React, { useEffect, useMemo, useState } from 'react';
import { Table, Button, Space, Modal, Form, Input, Select, Switch, message, Popconfirm, Card, Typography, Tag, Tooltip } from 'antd';
import { PlusOutlined, EditOutlined, DeleteOutlined, ApiOutlined } from '@ant-design/icons';
import request from '../../utils/request';

const { Title, Text, Link } = Typography;

interface BindingRow {
  id: number;
  name: string;
  channel_config_id: number;
  asset_base_path: string;
  asset_api_profile?: string | null;
  group_id?: string | null;
  is_active: number;
  /** 鉴权模式：bearer（默认，渠道 API Key）/ volc_v4（火山 V4 签名 AK/SK） */
  auth_mode?: string | null;
  /** V4 签名 AccessKey（SecretKey 出于安全不回传） */
  access_key?: string | null;
  sign_region?: string | null;
  sign_service?: string | null;
  /** 适用用户等级 ID 列表 */
  level_ids?: number[];
  remark?: string | null;
  created_at?: string | null;
  updated_at?: string | null;
  channel_name?: string | null;
  channel_base_url?: string | null;
  channel_status?: number | null;
}

interface ChannelConfigOption {
  id: number;
  name: string;
  base_url: string;
}

interface UserLevelOption {
  id: number;
  name: string;
  group_key: string;
}

// 描述符示例模板：非火山上游（如平行幻帧/cmcc）的请求/响应双向适配声明，
// 完整字段说明见后端 relay/asset_api_profile.rs
const PROFILE_TEMPLATE = JSON.stringify({
  actions: {
    CreateAssetGroup: {
      method: 'POST',
      path: '/v1/video/assets/groups',
      inject: { provider: 'cmcc', groupType: 'AIGC' },
      rename: { Name: 'groupName', Description: 'description' },
      response: { unwrap_body: true, ok_path: 'state', ok_value: 'OK', id_path: 'groupId' },
    },
    GetAsset: {
      method: 'GET',
      path: '/v1/video/assets/{assetId}',
      path_params: { assetId: 'Id' },
      response: { unwrap_body: true, ok_path: 'state', ok_value: 'OK', raw_result: true },
    },
    ListAssets: {
      method: 'POST',
      path: '/v1/video/assets/list',
      defaults: { pageNo: 1, pageSize: 20, statuses: ['ACTIVE'] },
      rename: { PageNumber: 'pageNo', PageSize: 'pageSize' },
      keep: ['provider', 'assetName', 'statuses', 'pageNo', 'pageSize'],
      response: {
        unwrap_body: true, ok_path: 'state', ok_value: 'OK',
        list: { items_path: 'data', item_id_field: 'assetId', total_path: 'total', target_key: 'Assets' },
      },
    },
  },
  unsupported: ['UpdateAsset'],
}, null, 2);

const UpstreamAssetBindings: React.FC = () => {
  const [bindings, setBindings] = useState<BindingRow[]>([]);
  const [channelConfigs, setChannelConfigs] = useState<ChannelConfigOption[]>([]);
  const [levels, setLevels] = useState<UserLevelOption[]>([]);
  const [loading, setLoading] = useState(true);
  const [modalVisible, setModalVisible] = useState(false);
  const [editing, setEditing] = useState<BindingRow | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [testingId, setTestingId] = useState<number | null>(null);
  const [form] = Form.useForm();

  const fetchBindings = async () => {
    setLoading(true);
    try {
      const resp = await (request.get('/upstream-asset-bindings') as unknown as Promise<{ data: BindingRow[] }>);
      setBindings(resp.data || []);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  };

  const fetchChannelConfigs = async () => {
    try {
      const resp = await (request.get('/channel-configs') as unknown as Promise<{ data: ChannelConfigOption[] }>);
      setChannelConfigs(resp.data || []);
    } catch (e) {
      console.error(e);
    }
  };

  const fetchLevels = async () => {
    try {
      const resp = await (request.get('/user_levels') as unknown as Promise<{ data: UserLevelOption[] }>);
      setLevels(resp.data || []);
    } catch (e) {
      console.error(e);
    }
  };

  useEffect(() => {
    fetchBindings();
    fetchChannelConfigs();
    fetchLevels();
  }, []);

  // 已被其他绑定占用的等级：从下拉里排除，用交互约束杜绝「一个等级绑多个上游」。
  // 编辑时要跳过当前绑定自身，否则它已占用的等级会消失、无法回显
  const takenLevelIds = useMemo(() => {
    const taken = new Set<number>();
    bindings.forEach((b) => {
      if (editing && b.id === editing.id) return;
      (b.level_ids || []).forEach((lid) => taken.add(lid));
    });
    return taken;
  }, [bindings, editing]);

  const levelOptions = useMemo(
    () => levels
      .filter((l) => !takenLevelIds.has(l.id))
      .map((l) => ({ value: l.id, label: `${l.name}（${l.group_key}）` })),
    [levels, takenLevelIds],
  );

  const levelName = (lid: number) => levels.find((l) => l.id === lid)?.name || `#${lid}`;

  const openCreate = () => {
    setEditing(null);
    form.resetFields();
    form.setFieldsValue({
      asset_base_path: '', asset_api_profile: '', level_ids: [],
      auth_mode: 'bearer', access_key: '', secret_key: '', sign_region: '', sign_service: '',
    });
    setModalVisible(true);
  };

  const openEdit = (record: BindingRow) => {
    setEditing(record);
    form.setFieldsValue({
      name: record.name,
      channel_config_id: record.channel_config_id,
      asset_base_path: record.asset_base_path,
      asset_api_profile: record.asset_api_profile || '',
      auth_mode: record.auth_mode || 'bearer',
      access_key: record.access_key || '',
      // SecretKey 不回显：留空 = 不修改（提交时剔除该字段）
      secret_key: '',
      sign_region: record.sign_region || '',
      sign_service: record.sign_service || '',
      level_ids: record.level_ids || [],
      remark: record.remark || '',
    });
    setModalVisible(true);
  };

  const handleSubmit = async () => {
    try {
      const values = await form.validateFields();
      // 多选 Select 直接产出 number[]，后端收 Vec<i64>
      const payload: any = {
        ...values,
        level_ids: values.level_ids || [],
      };
      // 编辑时 SecretKey 留空 = 不修改（后端 None 保留原值）；空串会被后端当作清空，故剔除
      if (editing && !payload.secret_key) {
        delete payload.secret_key;
      }
      setSubmitting(true);
      if (editing) {
        await request.put(`/upstream-asset-bindings/${editing.id}`, payload);
        message.success('已更新');
      } else {
        await request.post('/upstream-asset-bindings', payload);
        message.success('已创建');
      }
      setModalVisible(false);
      fetchBindings();
    } catch (e: any) {
      if (e?.errorFields) return; // 表单校验错误
      message.error(e?.response?.data?.error || '操作失败');
    } finally {
      setSubmitting(false);
    }
  };

  const handleDelete = async (id: number) => {
    try {
      await request.delete(`/upstream-asset-bindings/${id}`);
      message.success('已删除');
      fetchBindings();
    } catch (e: any) {
      message.error(e?.response?.data?.error || '删除失败');
    }
  };

  const handleToggle = async (record: BindingRow, checked: boolean) => {
    try {
      await request.put(`/upstream-asset-bindings/${record.id}`, { is_active: checked ? 1 : 0 });
      fetchBindings();
    } catch (e: any) {
      message.error(e?.response?.data?.error || '操作失败');
    }
  };

  const handleTest = async (record: BindingRow) => {
    setTestingId(record.id);
    try {
      const resp = await (request.post(`/upstream-asset-bindings/${record.id}/test`) as unknown as Promise<{ ok: boolean; latency_ms: number; error?: string }>);
      if (resp.ok) {
        message.success(`连通正常（${resp.latency_ms}ms）`);
      } else {
        message.error(`连通失败：${resp.error || '未知错误'}`);
      }
    } catch (e: any) {
      message.error(e?.response?.data?.error || '测试失败');
    } finally {
      setTestingId(null);
    }
  };

  const columns = [
    {
      title: 'ID',
      dataIndex: 'id',
      width: 70,
      render: (id: number) => <Text code>{id}</Text>,
    },
    {
      title: '名称',
      dataIndex: 'name',
      width: 170,
      render: (v: string) => <span>{v}</span>,
    },
    {
      title: '适用用户等级',
      dataIndex: 'level_ids',
      width: 200,
      render: (ids?: number[]) => (ids && ids.length ? (
        <Space size={4} wrap>
          {ids.map((lid) => <Tag key={lid} color="geekblue">{levelName(lid)}</Tag>)}
        </Space>
      ) : (
        <Tooltip title="未指定等级：该绑定不会被 /api?Action= 素材请求命中（素材路由只按用户等级解析）；仍可用于视频生成转发规则显式指定">
          <Text type="secondary">未指定</Text>
        </Tooltip>
      )),
    },
    {
      title: '上游渠道',
      dataIndex: 'channel_config_id',
      render: (_: any, record: BindingRow) => (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          <Space size={4}>
            <span>{record.channel_name || `#${record.channel_config_id}`}</span>
            {record.channel_status === 0 && <Tag color="red">渠道已停用</Tag>}
          </Space>
          <Text type="secondary" style={{ fontSize: 12 }}>{record.channel_base_url || '-'}</Text>
        </div>
      ),
    },
    {
      title: '素材接口地址',
      dataIndex: 'asset_base_path',
      width: 220,
      render: (v: string) => (v ? <Text code>{v}</Text> : <Text type="secondary">跟随渠道 base_url</Text>),
    },
    {
      title: '协议适配',
      dataIndex: 'asset_api_profile',
      width: 100,
      render: (v?: string | null) => (v && v.trim() ? (
        <Tooltip title="已配置 API 协议描述符，透传时按描述符做请求/响应双向适配">
          <Tag color="blue">描述符</Tag>
        </Tooltip>
      ) : (
        <Tooltip title="未配置描述符，按火山官方素材协议直接透传">
          <Text type="secondary">火山直透</Text>
        </Tooltip>
      )),
    },
    {
      title: '鉴权',
      dataIndex: 'auth_mode',
      width: 100,
      render: (v: string | null | undefined, record: BindingRow) => ((v || 'bearer') === 'volc_v4' ? (
        <Tooltip title={`火山 V4 签名（AK: ${record.access_key || '-'}，Region: ${record.sign_region || 'cn-beijing'}，Service: ${record.sign_service || 'ark'}）`}>
          <Tag color="purple">V4 签名</Tag>
        </Tooltip>
      ) : (
        <Tooltip title="Bearer：使用渠道配置的 API Key">
          <Text type="secondary">Bearer</Text>
        </Tooltip>
      )),
    },
    {
      title: '上游素材组',
      dataIndex: 'group_id',
      width: 180,
      render: (v?: string | null) => (v ? (
        <Tooltip title="系统自动创建并回写的上游素材组，用于素材转换时的归属">
          <Text code copyable>{v}</Text>
        </Tooltip>
      ) : <Text type="secondary">未创建</Text>),
    },
    {
      title: '状态',
      dataIndex: 'is_active',
      width: 80,
      render: (v: number, record: BindingRow) => (
        <Switch size="small" checked={v === 1} onChange={(checked) => handleToggle(record, checked)} />
      ),
    },
    {
      title: '备注',
      dataIndex: 'remark',
      ellipsis: true,
      render: (v?: string | null) => v || <Text type="secondary">-</Text>,
    },
    {
      title: '操作',
      width: 220,
      render: (_: any, record: BindingRow) => (
        <Space size={4}>
          <Button
            size="small"
            icon={<ApiOutlined />}
            loading={testingId === record.id}
            onClick={() => handleTest(record)}
          >
            测试
          </Button>
          <Button size="small" icon={<EditOutlined />} onClick={() => openEdit(record)} />
          <Popconfirm title="确认删除该绑定？" onConfirm={() => handleDelete(record.id)}>
            <Button size="small" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <Card style={{ margin: '0 16px 16px' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginBottom: 16, flexWrap: 'wrap', gap: 8 }}>
        <div>
          <Title level={4} style={{ marginBottom: 4 }}>上游素材绑定</Title>
          <Text type="secondary" style={{ fontSize: 13 }}>
            指定哪个上游渠道承担火山素材接口（/api?Action=CreateAsset 等）透传。
            每条绑定可直接声明适用的用户等级：请求时按「等级命中 → 默认绑定」依次解析，不再依赖渠道优先级。
          </Text>
        </div>
        <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>新建绑定</Button>
      </div>

      <Table
        rowKey="id"
        columns={columns}
        dataSource={bindings}
        loading={loading}
        pagination={{ pageSize: 20, showSizeChanger: true }}
        scroll={{ x: 1300 }}
      />

      <Modal
        title={editing ? '编辑绑定' : '新建绑定'}
        open={modalVisible}
        onCancel={() => setModalVisible(false)}
        onOk={handleSubmit}
        confirmLoading={submitting}
        destroyOnClose
      >
        <Form form={form} layout="vertical" style={{ marginTop: 16 }}>
          <Form.Item
            name="name"
            label="名称"
            rules={[{ required: true, message: '请输入名称' }]}
          >
            <Input placeholder="如：长夏" maxLength={64} />
          </Form.Item>
          <Form.Item
            name="channel_config_id"
            label="上游渠道"
            rules={[{ required: true, message: '请选择上游渠道' }]}
            extra="凭证（base_url + API Key）读取自该渠道配置，绑定本身不存储密钥"
          >
            <Select
              showSearch
              placeholder="选择上游渠道"
              optionFilterProp="label"
              options={channelConfigs.map((c) => ({
                value: c.id,
                label: `${c.name}（${c.base_url}）`,
              }))}
            />
          </Form.Item>
          <Form.Item
            name="auth_mode"
            label="鉴权模式"
            initialValue="bearer"
            extra="Bearer：用渠道配置的 API Key（存量默认）；火山 V4 签名：用 AK/SK 对请求做 Signature V4（乐信等以 ark 协议鉴权、域名非火山官方的上游；Region 默认 cn-beijing，Service 默认 ark）"
          >
            <Select
              options={[
                { value: 'bearer', label: 'Bearer（渠道 API Key）' },
                { value: 'volc_v4', label: '火山 V4 签名（AK/SK）' },
              ]}
            />
          </Form.Item>
          <Form.Item noStyle shouldUpdate={(prev, cur) => prev.auth_mode !== cur.auth_mode}>
            {({ getFieldValue }) => (getFieldValue('auth_mode') === 'volc_v4' ? (
              <>
                <Form.Item
                  name="access_key"
                  label="AccessKey"
                  rules={[{ required: true, message: '请输入 AccessKey' }]}
                >
                  <Input placeholder="V4 签名 AccessKey" maxLength={128} autoComplete="off" />
                </Form.Item>
                <Form.Item
                  name="secret_key"
                  label="SecretKey"
                  rules={editing ? [] : [{ required: true, message: '请输入 SecretKey' }]}
                  extra="出于安全不回显：编辑时留空 = 保留原值，填写 = 覆盖"
                >
                  <Input.Password placeholder="V4 签名 SecretKey" maxLength={128} autoComplete="new-password" />
                </Form.Item>
                <Space style={{ display: 'flex' }} align="start">
                  <Form.Item name="sign_region" label="签名 Region" initialValue="cn-beijing">
                    <Input placeholder="cn-beijing" maxLength={64} />
                  </Form.Item>
                  <Form.Item name="sign_service" label="签名 Service" initialValue="ark">
                    <Input placeholder="ark" maxLength={64} />
                  </Form.Item>
                </Space>
              </>
            ) : null)}
          </Form.Item>
          <Form.Item
            name="level_ids"
            label="适用用户等级"
            extra="一个绑定可挂多个等级；已被其他绑定占用的等级不会出现在下拉里（一个等级只能走一个素材上游）。留空 = 该绑定不会被素材请求命中，仅用于视频生成链路显式指定"
          >
            <Select
              mode="multiple"
              allowClear
              showSearch
              optionFilterProp="label"
              placeholder="选择适用该绑定的用户等级（可多选）"
              options={levelOptions}
              notFoundContent="所有等级已被其他绑定占用"
            />
          </Form.Item>
          <Form.Item
            name="asset_base_path"
            label="素材接口地址"
            extra="留空 = 与渠道 base_url 同域；填相对路径（如 /ark）= 拼在 base_url 之后；填完整 URL（如 https://mintel.591ll.com/render/api）= 素材域名与视频域名分离，API Key 仍取自该渠道配置"
            rules={[{
              validator: (_, value: string) => {
                const v = (value ?? '').trim();
                if (!v || v.startsWith('/') || /^https?:\/\//i.test(v)) return Promise.resolve();
                return Promise.reject(new Error('请填写以 / 开头的相对路径，或以 http:// / https:// 开头的完整地址'));
              },
            }]}
          >
            <Input placeholder="留空 / /ark / https://素材域名/路径" maxLength={255} />
          </Form.Item>
          <Form.Item
            name="asset_api_profile"
            label="API 协议描述符（JSON）"
            rules={[{
              validator: (_, value) => {
                if (!value || !value.trim()) return Promise.resolve();
                try {
                  const obj = JSON.parse(value);
                  if (typeof obj !== 'object' || obj === null || Array.isArray(obj)) {
                    return Promise.reject(new Error('描述符顶层必须是 JSON 对象'));
                  }
                  return Promise.resolve();
                } catch {
                  return Promise.reject(new Error('JSON 格式不合法'));
                }
              },
            }]}
            extra={
              <span>
                非火山协议的上游（如平行幻帧/cmcc）在此填入声明式描述符，透传时自动做请求/响应双向适配；
                留空 = 保持火山协议直透。保存后立即生效，无需重启。{' '}
                <Link onClick={() => form.setFieldsValue({ asset_api_profile: PROFILE_TEMPLATE })}>填入示例模板</Link>
              </span>
            }
          >
            <Input.TextArea
              rows={10}
              placeholder='{"actions": {"CreateAsset": {"method": "POST", "path": "...", ...}}, "unsupported": []}'
              style={{ fontFamily: 'monospace', fontSize: 12 }}
            />
          </Form.Item>
          <Form.Item name="remark" label="备注">
            <Input.TextArea rows={2} maxLength={255} />
          </Form.Item>
        </Form>
      </Modal>
    </Card>
  );
};

export default UpstreamAssetBindings;
