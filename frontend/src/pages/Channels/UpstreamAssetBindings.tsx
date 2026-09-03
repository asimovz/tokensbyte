/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

import React, { useEffect, useState } from 'react';
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

  useEffect(() => {
    fetchBindings();
    fetchChannelConfigs();
  }, []);

  const openCreate = () => {
    setEditing(null);
    form.resetFields();
    form.setFieldsValue({ asset_base_path: '', asset_api_profile: '' });
    setModalVisible(true);
  };

  const openEdit = (record: BindingRow) => {
    setEditing(record);
    form.setFieldsValue({
      name: record.name,
      channel_config_id: record.channel_config_id,
      asset_base_path: record.asset_base_path,
      asset_api_profile: record.asset_api_profile || '',
      remark: record.remark || '',
    });
    setModalVisible(true);
  };

  const handleSubmit = async () => {
    try {
      const values = await form.validateFields();
      setSubmitting(true);
      if (editing) {
        await request.put(`/upstream-asset-bindings/${editing.id}`, values);
        message.success('已更新');
      } else {
        await request.post('/upstream-asset-bindings', values);
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
      width: 150,
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
      title: '素材接口路径',
      dataIndex: 'asset_base_path',
      width: 150,
      render: (v: string) => (v ? <Text code>{v}</Text> : <Text type="secondary">根路径（?Action= 直收）</Text>),
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
            用户按分组/优先级命中渠道后自动路由到对应绑定；未配置时首次使用会自动创建绑定。
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
        scroll={{ x: 1000 }}
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
            name="asset_base_path"
            label="素材接口路径"
            extra="拼接在渠道 base_url 之后；留空表示上游在根路径直接接收 ?Action=（如 https://xxx/ark/?Action=... 则留空）"
          >
            <Input placeholder="留空 = 根路径；否则填如 /ark" maxLength={255} />
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
