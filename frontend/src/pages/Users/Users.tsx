/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia 
 * @license        MIT (https://www.tokensbyte.ai/)
 */

import React, { useEffect, useState, useMemo, startTransition } from 'react';
import { Table, Button, Space, Tag, Modal, Form, Input, InputNumber, message, Popconfirm, Card, Typography, Select, Progress, Grid, Radio, Tabs, Timeline, Row, Col, Tooltip, DatePicker, Statistic, Spin, Switch } from 'antd';
import MobileCardList, { MobileCard, CardRow, CardActions } from '../../components/MobileCardList';
import ModelSelector from '../../components/ModelSelector';
import WalletBalanceDisplay from '../../components/WalletBalanceDisplay';
import WalletDetailsView from '../../components/WalletDetailsView';
import UserKycFormFields, {
  kycToFormValues,
  formValuesToKycPayload,
  KYC_STATUS_META,
} from '../../components/UserKycFormFields';
import { PlusOutlined, EditOutlined, DeleteOutlined, UserOutlined, SyncOutlined, WalletOutlined, LoginOutlined, ArrowLeftOutlined, CloseOutlined, SearchOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'react-router-dom';
import request from '../../utils/request';
import useSettingsStore from '../../store/settings';
import useAuthStore from '../../store/auth';
import { useThemeStore } from '../../store/theme';
import generateUUID from '../../utils/uuid';

import type { User, UserKyc, UserKycStatus } from '../../types';
import dayjs from 'dayjs';
import { formatApiDateTime } from '../../utils/timedisplay';
import { toAbsoluteDateParam } from '../../utils/dateRangeParams';
import { Resizable } from 'react-resizable';
import type { ResizeCallbackData } from 'react-resizable';

const ResizableTitle = (props: any) => {
  const { onResize, width, ...restProps } = props;
  const thRef = React.useRef<HTMLTableCellElement>(null);

  if (!width) {
    return <th {...restProps} />;
  }

  return (
    <Resizable
      width={width}
      height={0}
      handle={
        <span
          className="react-resizable-handle"
          onClick={(e) => {
            e.stopPropagation();
          }}
          style={{
            position: 'absolute',
            right: 0,
            bottom: 0,
            zIndex: 1,
            width: '10px',
            height: '100%',
            cursor: 'col-resize',
            display: 'block'
          }}
        />
      }
      onResize={(e, { size }) => {
        // Bypass React state for 60fps native performance during dragging
        if (thRef.current) {
          const index = Array.from(thRef.current.parentNode!.children).indexOf(thRef.current);
          const tableContainer = thRef.current.closest('.ant-table');
          if (tableContainer) {
            const colgroups = tableContainer.querySelectorAll('colgroup');
            colgroups.forEach(cg => {
              const col = cg.children[index] as HTMLElement;
              if (col) {
                col.style.width = `${Math.max(size.width, 80)}px`;
                col.style.minWidth = `${Math.max(size.width, 80)}px`;
              }
            });
          }
        }
      }}
      onResizeStop={onResize}
      draggableOpts={{ enableUserSelectHack: false }}
    >
      <th ref={thRef} {...restProps} style={{ ...restProps.style, position: 'relative' }} />
    </Resizable>
  );
};

const { Title, Text } = Typography;
const { Option } = Select;
const { useBreakpoint } = Grid;

// Helper: check if email is a real user-bound email (not a placeholder)
const isRealEmail = (email?: string) => !!email && !email.endsWith('@tokensbyte.local');

/** 模型折扣 Tag：悬停一眼看全模型名与倍率（沿用早期友好布局） */
function ModelDiscountHoverTag({
  modelDiscounts,
  models,
  compact,
}: {
  modelDiscounts?: string | null;
  models: any[];
  compact?: boolean;
}) {
  let entries: [string, number][] = [];
  try {
    if (modelDiscounts) entries = Object.entries(JSON.parse(modelDiscounts));
  } catch { /* ignore */ }
  if (!entries.length) return null;

  const tip = (
    <div style={{ maxHeight: 260, overflowY: 'auto' }}>
      {entries.map(([mid, discount]) => {
        const name = models.find((m) => m.mid === mid)?.name || mid;
        return (
          <div key={mid} style={{ display: 'flex', justifyContent: 'space-between', gap: 16, padding: '2px 0', fontSize: 12 }}>
            <span style={{ opacity: 0.85 }}>{name}</span>
            <span style={{ fontWeight: 500, flexShrink: 0, color: discount < 1 ? '#52c41a' : discount > 1 ? '#ff4d4f' : undefined }}>
              {discount}x
            </span>
          </div>
        );
      })}
    </div>
  );

  return (
    <Tooltip title={tip} placement="right" overlayStyle={{ maxWidth: 360 }}>
      <Tag color="orange" style={{ alignSelf: 'flex-start', marginTop: compact ? 0 : 4, marginInlineEnd: 0, fontSize: 11, cursor: 'default', padding: '0 6px', lineHeight: '20px' }}>
        {compact ? `已设(${entries.length})` : `模型折扣(${entries.length})`}
      </Tag>
    </Tooltip>
  );
}

const Users: React.FC = () => {
  const { themeMode } = useThemeStore();
  const _isLight = themeMode === 'light';
  const { t } = useTranslation();
  const location = useLocation();
  const screens = useBreakpoint();
  const isAdminPage = location.pathname.includes('/admins');
  const targetRole = isAdminPage ? 'admin' : 'user';
  // 当前登录者是否为超级管理员（库内口径：role=admin 且未绑定管理员分组），决定管理员行是否展示删除入口
  const { user: currentUser } = useAuthStore();
  const isSuperAdminOperator = currentUser?.role === 'admin' && !currentUser?.admin_group_id;
  // 删除入口可见性：普通用户行始终保留；管理员行仅超级管理员可见，且超级管理员自己的行不展示，后端同步强校验
  const canDeleteRow = (record: User) => {
    if (record.role !== 'admin') return true;
    if (!isSuperAdminOperator) return false;
    return !!record.admin_group_id;
  };
  
  const { settings } = useSettingsStore();
  const currencySymbol = settings?.currency?.currency_symbol || '$';
  const [allUsers, setAllUsers] = useState<User[]>([]);
  const [users, setUsers] = useState<User[]>([]);
  const [userLevels, setUserLevels] = useState<any[]>([]);
  const [adminGroups, setAdminGroups] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [isModalVisible, setIsModalVisible] = useState(false);
  const [selectedRole, setSelectedRole] = useState<string>('user');
  const [editingUser, setEditingUser] = useState<User | null>(null);
  const [isRechargeModalVisible, setIsRechargeModalVisible] = useState(false);
  const [rechargingUser, setRechargingUser] = useState<User | null>(null);
  const [rechargeLoading, setRechargeLoading] = useState(false);

  const [walletTimeFilter, setWalletTimeFilter] = useState<'all' | 'month'>(() => {
    return (localStorage.getItem('walletTimeFilter') as 'all' | 'month') || 'month';
  });
  const [monthConsumptionMap, setMonthConsumptionMap] = useState<Record<string, { system_cost: number; gift_cost: number }>>({});

  useEffect(() => {
    if (users.length === 0 || walletTimeFilter !== 'month') {
      setMonthConsumptionMap({});
      return;
    }
    const userIds = users.map(u => u.id);
    request.post('/users/consumption/stats_batch', {
      user_ids: userIds,
      start_date: toAbsoluteDateParam(dayjs().startOf('month')),
      end_date: toAbsoluteDateParam(dayjs().endOf('month'), true),
    }, { skipErrorHandler: true } as any).then((res: any) => {
      setMonthConsumptionMap(res || {});
    }).catch((e) => {
      console.error('Failed to fetch month consumption stats', e);
      setMonthConsumptionMap({});
    });
  }, [walletTimeFilter, users]);

  const [columnsWidths, setColumnsWidths] = useState<Record<string, number>>({
    uid: 100,
    username: 320,
    registration_info: 280,
    user_group: 150,
    balance: 320,
    actions: 120,
  });

  const handleResize = (key: string) => (_e: React.SyntheticEvent<Element>, { size }: ResizeCallbackData) => {
    setColumnsWidths((prev) => ({ ...prev, [key]: Math.max(size.width, 80) }));
  };
  const [searchText, setSearchText] = useState('');
  const [filterGroup, setFilterGroup] = useState<string>('all');
  const [filterReferrer, setFilterReferrer] = useState('');
  /** 排序：default / 系统钱包余额 / 赠送钱包余额 / 信控额度 / 消费合计 */
  const [listSort, setListSort] = useState<'default' | 'balance_desc' | 'balance_asc' | 'gift_balance_desc' | 'gift_balance_asc' | 'credit_limit_desc' | 'credit_limit_asc' | 'consumption_desc' | 'consumption_asc'>('default');
  const [form] = Form.useForm();
  const [rechargeForm] = Form.useForm();
  const rechargeActionType = Form.useWatch('actionType', rechargeForm);
  const rechargeWalletType = Form.useWatch('walletType', rechargeForm) || 'system';

  // ── 钱包明细弹窗状态 ──
  const [walletDetailUser, setWalletDetailUser] = useState<User | null>(null);
  const [walletRecharges, setWalletRecharges] = useState<any[]>([]);
  const [walletDetailLoading, setWalletDetailLoading] = useState(false);
  // 缓存：{ userId: { data: [...], time: timestamp } }
  const walletCacheRef = React.useRef<Record<string, { data: any[]; time: number }>>({});
  const WALLET_CACHE_TTL = 30 * 1000; // 30秒

  // ── 模型折扣 Tab 相关状态 ──
  const [availableModels, setAvailableModels] = useState<any[]>([]);
  /** 当前编辑用户的模型折扣数据: {mid: discount} */
  const [discountMap, setDiscountMap] = useState<Record<string, number>>({});
  /** 已选用于设置折扣的模型 mid 列表 */
  const [discountMids, setDiscountMids] = useState<string[]>([]);

  // ── 等级变更历史 ──
  const [levelLogs, setLevelLogs] = useState<any[]>([]);
  const [levelLogsLoading, setLevelLogsLoading] = useState(false);
  // ── 用户实名 KYC ──
  const [userKyc, setUserKyc] = useState<UserKyc | null>(null);
  const [kycLoading, setKycLoading] = useState(false);
  const [kycSaving, setKycSaving] = useState(false);

  const fetchUsers = async () => {
    setLoading(true);
    try {
      const resp = await (request.get('/users') as unknown as Promise<{ data: User[] }>);
      setAllUsers(resp.data);
      // Filter by role
      const filteredUsers = resp.data.filter(u => u.role === targetRole);
      setUsers(filteredUsers);
      
      const levelsResp = await (request.get('/user_levels') as unknown as Promise<{ data: any[] }>);
      setUserLevels(levelsResp.data);

      const adminGroupsResp = await (request.get('/admin_groups') as unknown as Promise<{ data: any[] }>);
      setAdminGroups(adminGroupsResp.data);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchUsers();
    // 加载模型列表用于折扣标识显示模型名称（仅首次）
    if (availableModels.length === 0) {
      request.get('/models').then((resp: any) => {
        if (resp?.data) setAvailableModels(resp.data);
      }).catch(() => {});
    }
  }, [isAdminPage]);

  const displayedUsers = useMemo(() => {
    let result = users;
    
    if (filterGroup && filterGroup !== 'all') {
      result = result.filter(user => user.user_group === filterGroup);
    }

    const trimmedFilterReferrer = filterReferrer.trim();
    if (trimmedFilterReferrer) {
      const lowerRef = trimmedFilterReferrer.toLowerCase();
      result = result.filter(user => {
        if (!user.referred_by) return false;
        const referrerObj = allUsers.find(u => u.id === user.referred_by || u.uid === user.referred_by || u.username === user.referred_by);
        if (referrerObj) {
          return referrerObj.uid?.toLowerCase().includes(lowerRef) || 
                 referrerObj.username?.toLowerCase().includes(lowerRef) || 
                 (referrerObj.nickname && referrerObj.nickname.toLowerCase().includes(lowerRef));
        }
        return user.referred_by.toLowerCase().includes(lowerRef);
      });
    }
    
    const trimmedSearchText = searchText.trim();
    if (trimmedSearchText) {
      const lower = trimmedSearchText.toLowerCase();
      result = result.filter(user => 
        user.username?.toLowerCase().includes(lower) ||
        user.uid?.toLowerCase().includes(lower) ||
        user.nickname?.toLowerCase().includes(lower) ||
        (isRealEmail(user.email) && user.email!.toLowerCase().includes(lower)) ||
        user.mobile?.toLowerCase().includes(lower) ||
        user.register_ip?.toLowerCase().includes(lower) ||
        user.admin_remark?.toLowerCase().includes(lower)
      );
    }

    if (listSort === 'balance_desc' || listSort === 'balance_asc') {
      result = [...result].sort((a, b) => {
        const balA = a.balance || 0;
        const balB = b.balance || 0;
        return listSort === 'balance_desc' ? balB - balA : balA - balB;
      });
    } else if (listSort === 'gift_balance_desc' || listSort === 'gift_balance_asc') {
      result = [...result].sort((a, b) => {
        const giftA = a.gift_balance || 0;
        const giftB = b.gift_balance || 0;
        return listSort === 'gift_balance_desc' ? giftB - giftA : giftA - giftB;
      });
    } else if (listSort === 'credit_limit_desc' || listSort === 'credit_limit_asc') {
      // 信控为 0 的用户不参与排序，固定排在有信控额度的用户之后
      const withCredit: typeof result = [];
      const noCredit: typeof result = [];
      for (const u of result) {
        if ((u.credit_limit || 0) > 0) withCredit.push(u);
        else noCredit.push(u);
      }
      withCredit.sort((a, b) => {
        const creditA = a.credit_limit || 0;
        const creditB = b.credit_limit || 0;
        return listSort === 'credit_limit_desc' ? creditB - creditA : creditA - creditB;
      });
      result = [...withCredit, ...noCredit];
    } else if (listSort === 'consumption_desc' || listSort === 'consumption_asc') {
      result = [...result].sort((a, b) => {
        const costA = walletTimeFilter === 'month'
          ? (monthConsumptionMap[a.id]?.system_cost || 0)
          : (a.used_quota || 0);
        const costB = walletTimeFilter === 'month'
          ? (monthConsumptionMap[b.id]?.system_cost || 0)
          : (b.used_quota || 0);
        return listSort === 'consumption_desc' ? costB - costA : costA - costB;
      });
    }
    
    return result;
  }, [users, searchText, filterGroup, filterReferrer, allUsers, listSort, walletTimeFilter, monthConsumptionMap]);

  const handleAdd = () => {
    setEditingUser(null);
    setSelectedRole(targetRole);
    form.resetFields();
    form.setFieldsValue({ role: targetRole, is_active: 1, balance: 0, gift_balance: 0, user_group: 'default' });
    // 清空模型折扣状态
    setDiscountMap({});
    setDiscountMids([]);
    setUserKyc(null);
    setIsModalVisible(true);
  };

  const loadUserKyc = (userId: string) => {
    setKycLoading(true);
    (request.get(`/users/${userId}/kyc`) as unknown as Promise<UserKyc>)
      .then((res) => {
        setUserKyc(res);
        form.setFieldsValue(kycToFormValues(res));
      })
      .catch(() => {
        setUserKyc(null);
      })
      .finally(() => setKycLoading(false));
  };

  const handleEdit = (record: User) => {
    setEditingUser(record);
    setIsModalVisible(true);
    // 加载等级变更记录
    setLevelLogs([]);
    setLevelLogsLoading(true);
    (request.get(`/users/${record.id}/level-logs`) as unknown as Promise<{ data: any[] }>)
      .then(res => setLevelLogs(res.data || []))
      .catch(() => {})
      .finally(() => setLevelLogsLoading(false));
    setSelectedRole(record.role);
    form.setFieldsValue({
      ...record,
      password: '', // Don't show password
    });
    // 初始化模型折扣数据
    const md: Record<string, number> = record.model_discounts ? (() => { try { return JSON.parse(record.model_discounts); } catch { return {}; } })() : {};
    setDiscountMap(md);
    setDiscountMids(Object.keys(md));
    if (!isAdminPage) {
      loadUserKyc(record.id);
    } else {
      setUserKyc(null);
    }
  };

  const handleSaveKyc = async () => {
    if (!editingUser) return;
    try {
      const kycType = form.getFieldValue('kyc_type') || 'personal';
      const idDocType = form.getFieldValue('id_doc_type');
      const validityType = form.getFieldValue('validity_type');
      const fields = [
        'kyc_type', 'status', 'validity_type', 'reject_reason', 'admin_remark',
      ];
      if (kycType === 'enterprise') {
        fields.push('company_name', 'business_license_url', 'tax_registration_url', 'legal_notarization_url');
      } else {
        fields.push('real_name', 'id_doc_type', 'id_doc_front_url');
        if (idDocType === 'id_card') fields.push('id_doc_back_url');
      }
      if (validityType === 'expire_date') fields.push('expire_at');
      const values = await form.validateFields(fields);
      setKycSaving(true);
      const payload = formValuesToKycPayload(values, true);
      const res = await (request.put(`/users/${editingUser.id}/kyc`, payload) as unknown as Promise<UserKyc>);
      setUserKyc(res);
      form.setFieldsValue(kycToFormValues(res));
      message.success('实名信息已保存');
    } catch (e: any) {
      if (e?.errorFields) return;
      console.error(e);
    } finally {
      setKycSaving(false);
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await request.delete(`/users/${id}`);
      message.success(t('common.success'));
      fetchUsers();
    } catch (e) {
      console.error(e);
    }
  };

  const handleSave = async (values: { [key: string]: unknown }) => {
    try {
      if (editingUser) {
        const payload = { ...values };
        if (!payload.password || (payload.password as string).trim() === '') {
          delete payload.password;
        }
        if (payload.referred_by === undefined) {
          payload.referred_by = "";
        }
        // 编辑模式下不发送 balance/gift_balance，避免并发覆盖充值操作
        delete payload.balance;
        delete payload.gift_balance;
        delete payload.gift_used_quota;
        delete payload.used_quota;
        // 实名字段由「保存实名信息」单独提交，勿混入用户更新
        [
          'kyc_type', 'status', 'real_name', 'id_doc_type', 'id_doc_front_url', 'id_doc_back_url',
          'company_name', 'business_license_url', 'tax_registration_url', 'legal_notarization_url',
          'validity_type', 'expire_at', 'reject_reason', 'admin_remark',
        ].forEach((k) => { delete payload[k]; });
        // 保存模型折扣：仅保留已设置折扣值的模型
        const validDiscounts: Record<string, number> = {};
        for (const mid of discountMids) {
          if (discountMap[mid] !== undefined && discountMap[mid] !== null) {
            validDiscounts[mid] = discountMap[mid];
          }
        }
        payload.model_discounts = Object.keys(validDiscounts).length > 0 ? JSON.stringify(validDiscounts) : '';
        await request.put(`/users/${editingUser.id}`, payload);
        message.success(t('common.success'));
      } else {
        const payload: any = { ...values, role: targetRole };
        // 创建时去掉值为0的余额字段，避免后端不必要的记录
        if (payload.balance === 0 || payload.balance === null) delete payload.balance;
        if (payload.gift_balance === 0 || payload.gift_balance === null) delete payload.gift_balance;
        await request.post('/users', payload);
        message.success(t('common.success'));
      }
      setIsModalVisible(false);
      fetchUsers();
    } catch (e) {
      console.error(e);
    }
  };

  const handleRechargeClick = (record: User) => {
    setRechargingUser(record);
    rechargeForm.setFieldsValue({
      walletType: 'system',
      actionType: 'increase',
      amount: '',
      remark: '',
    });
    setIsRechargeModalVisible(true);
  };

  const handleRechargeSave = async (values: any) => {
    if (!rechargingUser) return;
    if (rechargeLoading) return;
    setRechargeLoading(true);
    try {
      let finalAmount = Number(values.amount);
      if (values.actionType === 'decrease') {
        finalAmount = -finalAmount;
      }
      const payload = {
        amount: finalAmount,
        remark: values.remark,
        wallet_type: values.walletType || 'system',
      };
      await request.post(`/users/${rechargingUser.id}/recharge`, payload);
      message.success(t('users.recharge_success'));
      setIsRechargeModalVisible(false);
      // 充值后清除该用户的钱包明细缓存，确保下次查看时获取最新数据
      delete walletCacheRef.current[rechargingUser.id];
      fetchUsers();
    } catch (e) {
      console.error(e);
    } finally {
      setRechargeLoading(false);
    }
  };

  // ── 钱包明细：获取用户充值记录（30秒 TTL 缓存） ──
  const openWalletDetail = async (record: User) => {
    setWalletDetailUser(record);
    // 检查缓存是否有效（30秒内）
    const cached = walletCacheRef.current[record.id];
    if (cached && Date.now() - cached.time < WALLET_CACHE_TTL) {
      setWalletRecharges(cached.data);
      return;
    }
    setWalletDetailLoading(true);
    try {
      const res = await (request.get('/finance/recharges', { params: { user_id: record.id, per_page: 500 } }) as any);
      const data = res.data || [];
      setWalletRecharges(data);
      walletCacheRef.current[record.id] = { data, time: Date.now() };
    } catch (e) {
      console.error('获取充值记录失败', e);
      setWalletRecharges([]);
    } finally {
      setWalletDetailLoading(false);
    }
  };

  const handleImpersonate = async (record: User) => {
    try {
      const resp = await (request.post(`/users/${record.id}/impersonate`) as unknown as Promise<{ token: string; user: User }>);
      const { token, user } = resp;
      
      // 管理端与用户端同属一个 Vite SPA；本地 5173=商业版、5174=开源版，直接用当前 origin
      const baseUrl = window.location.origin;
      
      message.success(`正在打开用户端: ${user.username}`);
      
      // 用 localStorage 一次性交接，避免 JWT 出现在 URL
      const handoffKey = `imp_handoff_${generateUUID()}`;
      localStorage.setItem(handoffKey, token);
      window.open(`${baseUrl}/login?impersonate=1&handoff=${encodeURIComponent(handoffKey)}`, '_blank', 'noopener,noreferrer');
    } catch (e) {
      console.error(e);
      message.error('切换用户失败');
    }
  };

  const baseColumns: any[] = [
    {
      title: t('users.uid'),
      dataIndex: 'uid',
      key: 'uid',
      render: (text: string) => (
        <Text code style={{ color: '#fff', padding: '2px 6px', whiteSpace: 'nowrap', fontSize: 13 }}>{text}</Text>
      ),
    },
    {
      title: t('users.username'),
      dataIndex: 'username',
      key: 'username',
      width: 320,
      render: (text: string, record: User) => (
        <div style={{ display: 'flex', flexDirection: 'column' }}>
          <Space align="center" style={{ flexWrap: 'wrap' }}>
            <UserOutlined />
            <Text strong>{text}</Text>
            {record.nickname && <Text type="secondary">({record.nickname})</Text>}
            <Tag color={record.is_active ? 'success' : 'error'} style={{ fontSize: 11, padding: '0 4px', margin: 0, whiteSpace: 'nowrap' }}>
              {record.is_active ? t('common.active') : t('common.disabled')}
            </Tag>
          </Space>
          {record.referred_by && (() => {
            const referrer = allUsers.find(u => u.id === record.referred_by || u.uid === record.referred_by || u.username === record.referred_by);
            return (
              <Text type="secondary" style={{ fontSize: '12px', marginTop: 4 }}>
                推荐人: {referrer ? `${referrer.username} (UID: ${referrer.uid})` : record.referred_by}
              </Text>
            );
          })()}
          <div style={{ maxWidth: 280, display: 'flex', marginTop: 4 }}>
          <Text 
            type="secondary" 
            style={{ fontSize: '12px', width: '100%' }}
            ellipsis={{ tooltip: record.admin_remark || '添加备注' }}
            editable={{
              text: record.admin_remark || '',
              onChange: async (val) => {
                if (val === record.admin_remark) return;
                try {
                  await request.put(`/users/${record.id}`, { admin_remark: val });
                  setAllUsers(prev => prev.map(u => u.id === record.id ? { ...u, admin_remark: val } : u));
                  setUsers(prev => prev.map(u => u.id === record.id ? { ...u, admin_remark: val } : u));
                } catch (e) {
                  console.error('Failed to update remark:', e);
                  message.error('备注更新失败');
                }
              },
              tooltip: '点击编辑用户备注',
              triggerType: ['icon']
            }}
          >
            {record.admin_remark || '添加备注'}
          </Text>
          </div>
          <ModelDiscountHoverTag modelDiscounts={record.model_discounts} models={availableModels} />
        </div>
      ),
    },
    {
      title: '注册信息',
      key: 'registration_info',
      width: 280,
      render: (_: any, record: User) => (
        <Space direction="vertical" size={2} style={{ fontSize: '13px' }}>
          {isRealEmail(record.email) && <div style={{ display: 'flex' }}><Text type="secondary" style={{ whiteSpace: 'nowrap' }}>邮箱: </Text><Text type="secondary" style={{ marginLeft: 4, maxWidth: 220 }} ellipsis={{ tooltip: record.email }}>{record.email}</Text></div>}
          {record.mobile && <Text type="secondary">手机号: {record.mobile}</Text>}
          <Text type="secondary">注册 IP: {record.register_ip || '未知'}</Text>
          <Text type="secondary">加入时间: {formatApiDateTime(record.created_at)}</Text>
        </Space>
      ),
    },

    {
      title: t('users.group'),
      dataIndex: 'user_group',
      key: 'user_group',
      render: (group: string, record: User) => {
        if (record.role === 'admin') {
          const adminGroup = adminGroups.find(g => g.id === record.admin_group_id);
          return <Tag color="purple">{adminGroup ? adminGroup.name : '超级管理员'}</Tag>;
        }
        
        const level = userLevels.find(l => l.group_key === group);
        const levelName = level ? level.name : (group || 'default').toUpperCase();
        
        return (
          <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-start', gap: '4px' }}>
            <Tag color={group === 'vip' ? 'gold' : group === 'partner' ? 'cyan' : 'default'} style={{ margin: 0 }}>
              {levelName}
            </Tag>
            {level && (
              <Text type="secondary" style={{ fontSize: '12px' }}>
                折扣倍率: {level.discount}x
              </Text>
            )}
          </div>
        );
      },
    },
    {
      title: '钱包余额',
      key: 'balance',
      width: 320,
      render: (_: unknown, record: User) => (
        <WalletBalanceDisplay 
          record={record} 
          onWalletClick={openWalletDetail} 
          width={140} 
          gap={20} 
          showConsumption
          monthConsumption={walletTimeFilter === 'month' ? (monthConsumptionMap[record.id] || { system_cost: 0, gift_cost: 0 }) : undefined}
        />
      ),
    },

    {
      title: t('common.actions'),
      key: 'actions',
      render: (_: unknown, record: User) => (
        <Space>
          <Button 
            icon={<WalletOutlined />} 
            style={{ color: '#52c41a', borderColor: '#52c41a' }}
            onClick={() => handleRechargeClick(record)} 
            title="充值"
          />
          {!isAdminPage && (
            <Button 
              icon={<LoginOutlined />} 
              style={{ color: '#1677ff', borderColor: '#1677ff' }}
              onClick={() => handleImpersonate(record)}
              title="登录此用户"
            />
          )}
          <Button icon={<EditOutlined />} onClick={() => handleEdit(record)} />
          {canDeleteRow(record) && (
            <Popconfirm title={t('common.confirm_delete')} onConfirm={() => handleDelete(record.id)}>
              <Button icon={<DeleteOutlined />} danger />
            </Popconfirm>
          )}
        </Space>
      ),
    },
  ];

  const columns = baseColumns.map((col: any) => ({
    ...col,
    width: columnsWidths[col.key as string] || col.width || 150,
    onHeaderCell: (column: any) => ({
      width: column.width,
      onResize: handleResize(column.key as string),
    }),
  }));

  return (
    <Card variant="borderless">
      {!isModalVisible ? (
      <>
      <div style={{ display: 'flex', flexDirection: screens.xs ? 'column' : 'row', justifyContent: 'space-between', marginBottom: 12, gap: 12 }}>
        <Title level={screens.xs ? 4 : 2} style={{ margin: 0 }}>
          {isAdminPage ? t('menu.admin_list') : t('menu.user_list')}
        </Title>
        <Space wrap>
          {!isAdminPage && (
            <Select
              value={filterGroup}
              onChange={setFilterGroup}
              style={{ width: screens.xs ? '100%' : 200, fontSize: 12, height: 32 }}
              styles={{ popup: { root: { fontSize: 12 } } }}
              options={[
                { value: 'all', label: '全部用户等级' },
                ...userLevels.map(level => ({ value: level.group_key, label: `${level.name} (${level.discount}x)` }))
              ]}
            />
          )}
          {!isAdminPage && (
            <Select
              value={listSort}
              onChange={setListSort}
              style={{ width: screens.xs ? '100%' : 200, fontSize: 12, height: 32 }}
              styles={{ popup: { root: { fontSize: 12 } } }}
              options={[
                { value: 'default', label: '默认排序' },
                { value: 'balance_desc', label: '系统钱包余额 ↓' },
                { value: 'balance_asc', label: '系统钱包余额 ↑' },
                { value: 'gift_balance_desc', label: '赠送钱包余额 ↓' },
                { value: 'gift_balance_asc', label: '赠送钱包余额 ↑' },
                { value: 'credit_limit_asc', label: '信控额度 ↑' },
                { value: 'credit_limit_desc', label: '信控额度 ↓' },
                { value: 'consumption_desc', label: '消费合计 ↓' },
                { value: 'consumption_asc', label: '消费合计 ↑' },
              ]}
            />
          )}
          {!isAdminPage && (
            <Select
              value={walletTimeFilter}
              onChange={(v) => {
                setWalletTimeFilter(v);
                localStorage.setItem('walletTimeFilter', v);
              }}
              options={[
                { label: '当月数据', value: 'month' },
                { label: '全部数据', value: 'all' },
              ]}
              style={{ width: screens.xs ? '100%' : 140, fontSize: 12, height: 32 }}
              styles={{ popup: { root: { fontSize: 12 } } }}
            />
          )}
          <Input 
            prefix={<SearchOutlined style={{ color: 'var(--ant-color-text-quaternary, #bfbfbf)' }} />}
            placeholder="搜索用户名/ID/昵称/邮箱/手机号/IP/备注..." 
            allowClear 
            value={searchText}
            onChange={(e) => setSearchText(e.target.value)} 
            onBlur={() => setSearchText(searchText.trim())}
            onPressEnter={(e) => setSearchText((e.target as HTMLInputElement).value.trim())}
            style={{ width: screens.xs ? '100%' : 280, fontSize: 12, height: 32 }}
          />
          {!isAdminPage && (
            <Input 
              prefix={<SearchOutlined style={{ color: 'var(--ant-color-text-quaternary, #bfbfbf)' }} />}
              placeholder="搜索推荐人(UID/用户名/昵称)..." 
              allowClear 
              value={filterReferrer}
              onChange={(e) => setFilterReferrer(e.target.value)} 
              onBlur={() => setFilterReferrer(filterReferrer.trim())}
              onPressEnter={(e) => setFilterReferrer((e.target as HTMLInputElement).value.trim())}
              style={{ width: screens.xs ? '100%' : 220, fontSize: 12, height: 32 }}
            />
          )}
          {isAdminPage && <Button icon={<SyncOutlined />} onClick={fetchUsers} style={{ height: 32, borderRadius: 6, fontSize: 12 }}>{t('common.refresh')}</Button>}
          <Button type="primary" icon={<PlusOutlined />} onClick={handleAdd} style={{ height: 32, borderRadius: 6, fontSize: 12 }}>{isAdminPage ? '添加管理员' : '添加普通用户'}</Button>
        </Space>
      </div>

      {screens.xs ? (
        <MobileCardList
          dataSource={displayedUsers}
          loading={loading}
          rowKey="id"
          pagination={{ pageSize: 20, showTotal: (total: number) => `共 ${total} 条` }}
          renderCard={(record: any) => {
            const level = userLevels.find((l: any) => l.group_key === record.user_group);
            const levelName = level ? level.name : (record.user_group || 'default').toUpperCase();
            return (
              <MobileCard
                title={<Space><UserOutlined /><Text strong>{record.username}</Text>{record.nickname && <Text type="secondary">({record.nickname})</Text>}</Space>}
                extra={<Tag color={record.is_active ? 'success' : 'error'}>{record.is_active ? t('common.active') : t('common.disabled')}</Tag>}
              >
                <CardRow label="UID"><Text code style={{ color: '#fff', fontSize: 12 }}>{record.uid}</Text></CardRow>
                {isRealEmail(record.email) && <CardRow label="邮箱"><Text style={{ fontSize: 12 }}>{record.email}</Text></CardRow>}
                {record.mobile && <CardRow label="手机号"><Text style={{ fontSize: 12 }}>{record.mobile}</Text></CardRow>}
                
                {/* 推荐人 */}
                {record.referred_by && (() => {
                  const referrer = allUsers.find(u => u.id === record.referred_by || u.uid === record.referred_by || u.username === record.referred_by);
                  return (
                    <CardRow label="推荐人">
                      <Text type="secondary" style={{ fontSize: '12px' }}>
                        {referrer ? `${referrer.username} (UID: ${referrer.uid})` : record.referred_by}
                      </Text>
                    </CardRow>
                  );
                })()}

                {/* 备注 (可编辑) */}
                <CardRow label="备注">
                  <div style={{ maxWidth: 200, display: 'flex', justifyContent: 'flex-end' }}>
                    <Text 
                      type="secondary" 
                      style={{ fontSize: '12px', textAlign: 'right' }}
                      ellipsis={{ tooltip: record.admin_remark || '添加备注' }}
                      editable={{
                        text: record.admin_remark || '',
                        onChange: async (val) => {
                          if (val === record.admin_remark) return;
                          try {
                            await request.put(`/users/${record.id}`, { admin_remark: val });
                            setAllUsers(prev => prev.map(u => u.id === record.id ? { ...u, admin_remark: val } : u));
                            setUsers(prev => prev.map(u => u.id === record.id ? { ...u, admin_remark: val } : u));
                          } catch (e) {
                            console.error('Failed to update remark:', e);
                            message.error('备注更新失败');
                          }
                        },
                        tooltip: '点击编辑用户备注',
                        triggerType: ['icon']
                      }}
                    >
                      {record.admin_remark || '添加备注'}
                    </Text>
                  </div>
                </CardRow>

                {/* 模型单独折扣标识 */}
                {record.model_discounts && (
                  <CardRow label="模型折扣">
                    <ModelDiscountHoverTag modelDiscounts={record.model_discounts} models={availableModels} compact />
                  </CardRow>
                )}

                {isAdminPage ? (
                  <CardRow label="分组">
                    {(() => {
                      const adminGroup = adminGroups.find((g: any) => g.id === record.admin_group_id);
                      return <Tag color="purple">{adminGroup ? adminGroup.name : '超级管理员'}</Tag>;
                    })()}
                  </CardRow>
                ) : (
                  <CardRow label="等级">
                    <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-end', gap: '4px' }}>
                      <Tag color={record.user_group === 'vip' ? 'gold' : record.user_group === 'partner' ? 'cyan' : 'default'} style={{ margin: 0 }}>
                        {levelName}
                      </Tag>
                      {level && (
                        <Text type="secondary" style={{ fontSize: '12px' }}>
                          折扣倍率: {level.discount}x
                        </Text>
                      )}
                    </div>
                  </CardRow>
                )}

                <div style={{ padding: '8px 0', borderBottom: '1px solid var(--ant-color-border-secondary)', borderTop: '1px solid var(--ant-color-border-secondary)', marginTop: 8 }}>
                  <div style={{ marginBottom: 8 }}>
                    <Text type="secondary" style={{ fontSize: 13 }}>钱包数据</Text>
                  </div>
                  <WalletBalanceDisplay 
                    record={record} 
                    onWalletClick={openWalletDetail} 
                    showConsumption
                    monthConsumption={walletTimeFilter === 'month' ? (monthConsumptionMap[record.id] || { system_cost: 0, gift_cost: 0 }) : undefined}
                    gap={12}
                  />
                </div>
                <CardRow label="注册IP"><Text type="secondary" style={{ fontSize: 12 }}>{record.register_ip || '未知'}</Text></CardRow>
                <CardRow label="加入时间"><Text type="secondary" style={{ fontSize: 12 }}>{formatApiDateTime(record.created_at, 'MM-DD HH:mm')}</Text></CardRow>
                <CardActions>
                  <Button size="small" icon={<WalletOutlined />} style={{ color: '#52c41a', borderColor: '#52c41a' }} onClick={() => handleRechargeClick(record)} title="充值" />
                  {!isAdminPage && (
                    <Button size="small" icon={<LoginOutlined />} style={{ color: '#1677ff', borderColor: '#1677ff' }} onClick={() => handleImpersonate(record)} title="登录此用户" />
                  )}
                  <Button size="small" icon={<EditOutlined />} onClick={() => handleEdit(record)} />
                  {canDeleteRow(record) && (
                    <Popconfirm title={t('common.confirm_delete')} onConfirm={() => handleDelete(record.id)}>
                      <Button size="small" icon={<DeleteOutlined />} danger />
                    </Popconfirm>
                  )}
                </CardActions>
              </MobileCard>
            );
          }}
        />
      ) : (
        <Table
          components={{
            header: {
              cell: ResizableTitle,
            },
          }}
          className="compact-table"
          dataSource={displayedUsers}
          columns={columns}
          rowKey="id"
          loading={loading}
          size="small"
          pagination={{ 
            pageSize: 50, 
            pageSizeOptions: ['50', '100', '200'], 
            showSizeChanger: true,
            showTotal: (total) => `共 ${total} 条数据`
          }}
          scroll={{ x: 'max-content' }}
          showSorterTooltip={false}
        />
      )}
      </>
      ) : (
        <div style={{ animation: 'fadeIn 0.3s' }}>
          <div style={{ display: 'flex', alignItems: 'center', marginBottom: 12, gap: 16 }}>
            <Button icon={<ArrowLeftOutlined />} onClick={() => setIsModalVisible(false)}>返回</Button>
            <Title level={3} style={{ margin: 0 }}>
              {editingUser ? t('users.edit_user') : (isAdminPage ? '添加管理员' : '添加普通用户')}
            </Title>
          </div>
          <div style={{ maxWidth: 1200, width: '100%' }}>
            <Form form={form} layout="vertical" onFinish={handleSave}>
              <Tabs
                defaultActiveKey="1"
                items={[
                  {
                    key: '1',
                    label: '用户基本信息',
                    children: (
                      <>
          <Form.Item
            name="username"
            label={t('users.username')}
            rules={[
              { required: true, message: '请输入用户名' },
              {
                validator: (_, val) =>
                  (editingUser && val === editingUser.username) || !val || (val.length >= 5 && val.length <= 48)
                    ? Promise.resolve()
                    : Promise.reject(new Error(val.length < 5 ? '用户名长度不能少于 5 个字符' : '正确输入用户名限制为 48 字'))
              }
            ]}
          >
            <Input placeholder={t('users.username')} />
          </Form.Item>
          <Form.Item
            name="nickname"
            label="用户昵称"
            rules={[{ max: 24, message: '昵称长度最多不能超过 24 个字符' }]}
          >
            <Input placeholder="输入用户昵称" />
          </Form.Item>
          <Form.Item name="admin_remark" label="用户备注 (管理员可见)">
            <Input.TextArea placeholder="写入简便备注例如: vip客户" rows={3} autoSize={{ minRows: 2, maxRows: 6 }} />
          </Form.Item>
          <Form.Item name="referred_by" label="上级推荐人 (UID / User ID)">
            <Select
              showSearch
              allowClear
              placeholder="输入用户名、UID 或邮箱快速搜索"
              filterOption={(input, option) => {
                if (!option) return false;
                const searchStr = String(option.label || '').toLowerCase();
                return searchStr.includes(input.toLowerCase());
              }}
              options={allUsers.map(u => ({
                value: String(u.id),
                label: `${u.username} ${u.nickname ? `(${u.nickname})` : ''} - UID: ${u.uid || u.id} ${isRealEmail(u.email) ? `(${u.email})` : ''}`
              }))}
            />
          </Form.Item>
          <Form.Item name="mobile" label="手机号">
            <Input placeholder="输入用户手机号 (可选)" />
          </Form.Item>
          <Form.Item name="email" label={t('users.email')} rules={[{ required: true, type: 'email' }]}>
            <Input placeholder="email@example.com" />
          </Form.Item>
          <Form.Item 
            name="password" 
            label={editingUser ? t('users.password_hint') : t('login.password')}
            rules={[{ required: !editingUser }]}
          >
            <Input.Password placeholder={t('login.password')} />
          </Form.Item>
          {!editingUser && (
             <Form.Item name="role" label={t('users.role')} initialValue={targetRole} hidden>
                <Input />
             </Form.Item>
          )}
          {editingUser && isAdminPage && (
            <Form.Item name="role" label={t('users.role')}>
              <Select onChange={(val) => setSelectedRole(val)}>
                <Option value="user">User</Option>
                <Option value="admin">Admin</Option>
              </Select>
            </Form.Item>
          )}
          {selectedRole === 'admin' && (
            <Form.Item name="admin_group_id" label="管理员等级" tooltip="未分配则默认为全权限超级管理员">
              <Select placeholder="选择分组" allowClear>
                {adminGroups.map(group => (
                  <Option key={group.id} value={group.id}>{group.name}</Option>
                ))}
              </Select>
            </Form.Item>
          )}
          {!editingUser && (
            <div style={{ display: 'flex', gap: 16 }}>
              <Form.Item name="balance" label={`系统钱包余额 (${currencySymbol})`} initialValue={0} style={{ flex: 1 }}>
                <InputNumber style={{ width: '100%' }} precision={6} min={0} />
              </Form.Item>
              <Form.Item name="gift_balance" label={`赠送钱包余额 (${currencySymbol})`} initialValue={0} style={{ flex: 1 }}>
                <InputNumber style={{ width: '100%' }} precision={6} min={0} />
              </Form.Item>
            </div>
          )}
          {!isAdminPage && (
            <Form.Item name="user_group" label="普通用户等级" initialValue="default">
              <Select>
                {userLevels.map(level => (
                  <Option key={level.id} value={level.group_key}>
                    {level.name} (折扣倍率: {level.discount}x)
                  </Option>
                ))}
              </Select>
            </Form.Item>
          )}
          <Form.Item name="is_active" label={t('common.status')} initialValue={1}>
            <Select>
              <Option value={1}>{t('common.active')}</Option>
              <Option value={0}>{t('common.disabled')}</Option>
            </Select>
          </Form.Item>
                      </>
                    )
                  },
                  ...(editingUser ? [{
                    key: '2',
                    label: '用户详细',
                    children: (
                      <div style={{ display: 'flex', flexDirection: 'column', gap: 16, marginTop: 8 }}>
                        <div>
                          <Typography.Text type="secondary" style={{ display: 'block', marginBottom: 4 }}>注册时间:</Typography.Text>
                          <Typography.Text>{editingUser.created_at ? formatApiDateTime(editingUser.created_at) : '未知'}</Typography.Text>
                        </div>
                        <div>
                          <Typography.Text type="secondary" style={{ display: 'block', marginBottom: 4 }}>最后活跃时间:</Typography.Text>
                          <Typography.Text>{editingUser.updated_at ? formatApiDateTime(editingUser.updated_at) : '未知'}</Typography.Text>
                        </div>
                        <div>
                          <Typography.Text type="secondary" style={{ display: 'block', marginBottom: 4 }}>注册 IP:</Typography.Text>
                          <Typography.Text>{editingUser.register_ip || '未知'}</Typography.Text>
                        </div>
                        <div>
                          <Typography.Text type="secondary" style={{ display: 'block', marginBottom: 8 }}>关联记录 (流转记录):</Typography.Text>
                          <div style={{ padding: '16px 16px 0', background: 'var(--ant-color-fill-quaternary, rgba(0,0,0,0.02))', borderRadius: 8, minHeight: 100, border: '1px solid var(--ant-color-border-secondary, #f0f0f0)' }}>
                            {editingUser.referral_history ? (
                              <Timeline 
                                items={editingUser.referral_history.split('\n').filter(line => line.trim()).map(line => {
                                  const match = line.match(/^\[(.*?)\]\s*(.*)$/);
                                  if (match) {
                                    return {
                                      color: 'blue',
                                      children: (
                                        <>
                                          <Typography.Text type="secondary" style={{ fontSize: 12, display: 'block' }}>{match[1]}</Typography.Text>
                                          <Typography.Text style={{ marginTop: 2, display: 'block' }}>{match[2]}</Typography.Text>
                                        </>
                                      )
                                    };
                                  }
                                  return { children: <Typography.Text>{line}</Typography.Text> };
                                }).reverse()}
                              />
                            ) : (
                              <Typography.Text type="secondary">暂无流转记录</Typography.Text>
                            )}
                          </div>
                        </div>
                         {/* 等级变更记录（仅非管理员页面显示） */}
                         {!isAdminPage && (
                           <div>
                             <Typography.Text type="secondary" style={{ display: 'block', marginBottom: 8 }}>等级变更记录:</Typography.Text>
                             <div style={{ padding: '16px 16px 0', background: 'var(--ant-color-fill-quaternary, rgba(0,0,0,0.02))', borderRadius: 8, minHeight: 60, border: '1px solid var(--ant-color-border-secondary, #f0f0f0)' }}>
                               {levelLogsLoading ? (
                                 <div style={{ textAlign: 'center', padding: '12px 0' }}><Spin size="small" /></div>
                               ) : levelLogs.length === 0 ? (
                                 <Typography.Text type="secondary">暂无等级变更记录</Typography.Text>
                               ) : (
                                 <div style={{ maxHeight: 300, overflowY: 'auto' }}>
                                   <Timeline
                                     items={levelLogs.map((log: any) => {
                                       const sourceMap: Record<string, { label: string; color: string }> = {
                                         admin: { label: '管理员', color: 'blue' },
                                         marketing: { label: '推广负责人', color: 'purple' },
                                         system: { label: '系统自动', color: 'default' },
                                       };
                                       const src = sourceMap[log.source] || { label: log.source, color: 'default' };
                                       return {
                                         color: src.color === 'blue' ? 'blue' : src.color === 'purple' ? 'purple' : 'gray',
                                         children: (
                                           <div style={{ paddingBottom: 4 }}>
                                             <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap', marginBottom: 4 }}>
                                               <Text type="secondary" style={{ fontSize: 12 }}>
                                                 {formatApiDateTime(log.created_at)}
                                               </Text>
                                               <Tag color={src.color === 'blue' ? 'blue' : src.color === 'purple' ? 'purple' : 'default'} style={{ fontSize: 11, margin: 0 }}>
                                                 {src.label}
                                               </Tag>
                                             </div>
                                             <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}>
                                               <Tag color="default" style={{ fontSize: 12, margin: 0 }}>
                                                 {log.old_level_name || log.old_level || '—'}
                                               </Tag>
                                               <Text style={{ fontSize: 12 }}>→</Text>
                                               <Tag color="success" style={{ fontSize: 12, margin: 0 }}>
                                                 {log.new_level_name || log.new_level || '—'}
                                               </Tag>
                                             </div>
                                             <div style={{ marginTop: 4 }}>
                                               <Text type="secondary" style={{ fontSize: 12 }}>
                                                 操作人：{log.operator || '未知'}
                                                 {log.remark ? `  ·  ${log.remark}` : ''}
                                               </Text>
                                             </div>
                                           </div>
                                         ),
                                       };
                                     })}
                                   />
                                 </div>
                               )}
                             </div>
                           </div>
                         )}
                       </div>
                     )
                   },
                  {
                    key: '3',
                    label: '账号绑定',
                    children: (
                      <div style={{ display: 'flex', flexDirection: 'column', gap: 16, marginTop: 8 }}>
                        {[{
                          label: '📧 邮箱',
                          bound: isRealEmail(editingUser.email),
                          value: isRealEmail(editingUser.email) ? editingUser.email : null,
                        }, {
                          label: '📱 手机号',
                          bound: !!editingUser.mobile,
                          value: editingUser.mobile || null,
                        }, {
                          label: '🔵 Google',
                          bound: !!editingUser.google_id,
                          value: editingUser.google_name || editingUser.google_id || null,
                        }, {
                          label: '💬 微信',
                          bound: !!editingUser.wechat_id,
                          value: editingUser.wechat_name || editingUser.wechat_id || null,
                        }].map(item => (
                          <div key={item.label} style={{ padding: '12px 16px', borderRadius: 8, background: 'var(--ant-color-fill-quaternary, rgba(0,0,0,0.02))', border: '1px solid var(--ant-color-border-secondary, #f0f0f0)', display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                            <Space>
                              <Typography.Text strong>{item.label}</Typography.Text>
                              {item.bound && item.value && <Typography.Text type="secondary" style={{ fontSize: 13 }}>{item.value}</Typography.Text>}
                            </Space>
                            <Tag color={item.bound ? 'success' : 'default'}>{item.bound ? '已绑定' : '未绑定'}</Tag>
                          </div>
                        ))}
                      </div>
                    )
                  }] : []),
                  // ── 支付设置 Tab（仅非管理员页面显示） ──
                  ...(!isAdminPage ? [{
                    key: '5',
                    label: '支付设置',
                    children: (
                      <div style={{ marginTop: 8 }}>
                        <Typography.Text type="secondary" style={{ display: 'block', marginBottom: 16, fontSize: 13 }}>
                          控制该用户是否可使用在线充值功能。关闭后，用户端「我的钱包」将不显示在线充值按钮，且无法发起任何支付请求。
                        </Typography.Text>
                        <div style={{ padding: '16px 20px', background: _isLight ? '#f9fafb' : 'rgba(255,255,255,0.04)', borderRadius: 8, border: _isLight ? '1px solid #e5e4e7' : '1px solid rgba(255,255,255,0.1)' }}>
                          <Form.Item
                            name="pay_enabled"
                            label="在线支付"
                            initialValue={1}
                            style={{ marginBottom: 0 }}
                            extra="开启后用户可在「我的钱包」使用在线充值功能（支付宝、微信、Stripe、加密货币等）"
                          >
                            <Radio.Group buttonStyle="solid" optionType="button">
                              <Radio value={1}>✅ 允许支付</Radio>
                              <Radio value={0}>🚫 禁止支付</Radio>
                            </Radio.Group>
                          </Form.Item>
                        </div>
                      </div>
                    )
                  }] : []),
                  // ── 模型折扣 Tab（仅编辑模式 + 非管理员页面） ──
                  ...((editingUser && !isAdminPage) ? [{
                    key: '4',
                    label: '模型折扣',
                    children: (
                      <div style={{ marginTop: 8 }}>
                        <Typography.Text type="secondary" style={{ display: 'block', marginBottom: 16, fontSize: 13 }}>
                          为该用户针对特定模型设置单独折扣。系统取 MIN(用户模型折扣, 全站折扣, 等级折扣) 中最低值，若模型开启折扣限价则 MAX(最低折扣, 限价) 保底。
                        </Typography.Text>
                        <Row gutter={24}>
                          {/* 左侧：已选模型及折扣设置 */}
                          <Col xs={24} md={10}>
                            <div style={{ padding: 16, background: _isLight ? '#f9fafb' : 'rgba(255,255,255,0.04)', borderRadius: 8, minHeight: 300 }}>
                              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
                                <Text strong>已选模型折扣 ({discountMids.length})</Text>
                              </div>
                              {discountMids.length === 0 ? (
                                <Text type="secondary" style={{ fontSize: 13 }}>从右侧选择模型后设置折扣</Text>
                              ) : (
                                <div style={{ display: 'flex', flexDirection: 'column', gap: 8, maxHeight: 500, overflowY: 'auto' }}>
                                  {discountMids.map(mid => {
                                    const model = availableModels.find((m: any) => m.mid === mid);
                                    const discount = discountMap[mid];
                                    const siteDiscount = model?.site_discount_enabled ? model.site_discount : null;
                                    return (
                                      <div key={mid} style={{ padding: '8px 12px', background: _isLight ? '#fff' : 'rgba(255,255,255,0.06)', borderRadius: 6, border: _isLight ? '1px solid #e5e4e7' : '1px solid rgba(255,255,255,0.1)' }}>
                                        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6 }}>
                                          <div style={{ flex: 1, minWidth: 0 }}>
                                            <div style={{ fontWeight: 500, fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                                              {model ? model.name : mid}
                                              {model?.remark && <span style={{ color: 'var(--text-secondary)', fontWeight: 'normal', marginLeft: 4, fontSize: 11 }}>({model.remark})</span>}
                                            </div>
                                            <div style={{ fontSize: 11, color: 'var(--text-secondary)' }}>{model ? model.model_id : ''} · MID: {mid}</div>
                                          </div>
                                          <Button
                                            type="text"
                                            size="small"
                                            icon={<CloseOutlined style={{ fontSize: 10 }} />}
                                            style={{ width: 20, height: 20, minWidth: 20, color: 'var(--text-secondary)' }}
                                            onClick={() => {
                                              setDiscountMids(prev => prev.filter(id => id !== mid));
                                              setDiscountMap(prev => { const n = { ...prev }; delete n[mid]; return n; });
                                            }}
                                          />
                                        </div>
                                        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                                          <InputNumber
                                            size="small"
                                            min={siteDiscount !== null ? siteDiscount : 0}
                                            max={2}
                                            step={0.01}
                                            precision={3}
                                            value={discount}
                                            onChange={(val) => setDiscountMap(prev => ({ ...prev, [mid]: val ?? 1 }))}
                                            style={{ width: 100 }}
                                            placeholder="折扣倍率"
                                            addonAfter="x"
                                          />
                                          <Text type="secondary" style={{ fontSize: 11 }}>
                                            {discount !== undefined ? `${(discount * 100).toFixed(1)}%` : '未设置'}
                                          </Text>
                                          {siteDiscount !== null && (
                                            <Tag color="orange" style={{ margin: 0, fontSize: 10, lineHeight: '16px', padding: '0 4px' }}>
                                              限价 {siteDiscount}x
                                            </Tag>
                                          )}
                                        </div>
                                      </div>
                                    );
                                  })}
                                </div>
                              )}
                            </div>
                          </Col>
                          {/* 右侧：ModelSelector 选择模型 */}
                          <Col xs={24} md={14}>
                            <div style={{ padding: 16, background: _isLight ? '#fff' : 'rgba(255,255,255,0.02)', border: _isLight ? '1px solid #e5e4e7' : '1px solid rgba(255,255,255,0.08)', borderRadius: 8 }}>
                              <ModelSelector
                                selectedMids={discountMids}
                                onSelectionChange={(mids) => {
                                  setDiscountMids(mids);
                                }}
                                onModelsLoaded={(models) => setAvailableModels(models)}
                                allowDuplicateModelId={true}
                                isLightTheme={_isLight}
                                title="选择模型"
                              />
                            </div>
                          </Col>
                        </Row>
                      </div>
                    )
                  }] : []),
                  // ── 用户实名 Tab（仅编辑普通用户） ──
                  ...((editingUser && !isAdminPage) ? [{
                    key: '6',
                    label: (
                      <span>
                        用户实名
                        {userKyc?.status && userKyc.status !== 'none' ? (
                          <Tag
                            color={KYC_STATUS_META[(userKyc.status as UserKycStatus)]?.color || 'default'}
                            style={{ marginLeft: 8 }}
                          >
                            {KYC_STATUS_META[(userKyc.status as UserKycStatus)]?.label || userKyc.status}
                          </Tag>
                        ) : null}
                      </span>
                    ),
                    children: kycLoading ? (
                      <div style={{ textAlign: 'center', padding: 40 }}><Spin /></div>
                    ) : (
                      <div style={{ marginTop: 8 }}>
                        <Typography.Text type="secondary" style={{ display: 'block', marginBottom: 16, fontSize: 13 }}>
                          录入或审核该用户的个人/企业实名信息。证件图片需先配置对象存储（站点设置 → 存储）。
                        </Typography.Text>
                        <UserKycFormFields
                          form={form}
                          mode="admin"
                          targetUserId={editingUser.id}
                          currentStatus={(userKyc?.status as UserKycStatus) || 'none'}
                        />
                        <Button type="primary" loading={kycSaving} onClick={handleSaveKyc} style={{ marginTop: 8 }}>
                          保存实名信息
                        </Button>
                      </div>
                    )
                  }] : [])
                ]}
              />

              <div style={{ marginTop: 24, display: 'flex', gap: 12 }}>
                <Button type="primary" onClick={() => form.submit()}>保存</Button>
                <Button onClick={() => setIsModalVisible(false)}>取消</Button>
              </div>
            </Form>
          </div>
        </div>
      )}

      <Modal
        title={
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <WalletOutlined style={{ color: '#1677ff' }} />
            <span>{t('users.recharge')}</span>
          </div>
        }
        open={isRechargeModalVisible}
        onCancel={() => setIsRechargeModalVisible(false)}
        onOk={() => rechargeForm.submit()}
        confirmLoading={rechargeLoading}
        width={500}
        destroyOnClose
      >
        <div style={{ 
          padding: '16px 20px', 
          background: _isLight ? '#f8f9fa' : 'rgba(255,255,255,0.02)', 
          borderRadius: 12, 
          marginBottom: 24,
          border: `1px solid ${_isLight ? '#f0f0f0' : 'rgba(255,255,255,0.06)'}`,
          boxShadow: _isLight ? '0 2px 8px rgba(0,0,0,0.02)' : 'inset 0 1px 1px rgba(255,255,255,0.02)'
        }}>
          <div style={{ display: 'flex', alignItems: 'center', marginBottom: 16 }}>
            <div style={{ 
              width: 32, height: 32, borderRadius: '50%', 
              background: _isLight ? '#e6f4ff' : 'rgba(22,119,255,0.15)', 
              display: 'flex', alignItems: 'center', justifyContent: 'center', marginRight: 12 
            }}>
              <UserOutlined style={{ fontSize: 16, color: '#1677ff' }} />
            </div>
            <div>
              <Text type="secondary" style={{ fontSize: 12, display: 'block', lineHeight: 1.2 }}>{t('users.username')}</Text>
              <Text strong style={{ fontSize: 16, lineHeight: 1.2 }}>{rechargingUser?.username}</Text>
            </div>
          </div>
          
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: '16px 32px' }}>
            <div style={{ display: 'flex', flexDirection: 'column' }}>
              <Text type="secondary" style={{ fontSize: 12, marginBottom: 4 }}>系统钱包余额</Text>
              <Text strong style={{ color: '#1677ff', fontSize: 18, fontFamily: 'monospace' }}>{currencySymbol}{(rechargingUser?.balance || 0).toFixed(6)}</Text>
            </div>
            <div style={{ display: 'flex', flexDirection: 'column' }}>
              <Text type="secondary" style={{ fontSize: 12, marginBottom: 4 }}>赠送钱包余额</Text>
              <Text strong style={{ color: '#faad14', fontSize: 18, fontFamily: 'monospace' }}>🎁 {currencySymbol}{(rechargingUser?.gift_balance || 0).toFixed(6)}</Text>
            </div>
            {((rechargingUser?.credit_limit || 0) > 0) && (
              <div style={{ display: 'flex', flexDirection: 'column' }}>
                <Text type="secondary" style={{ fontSize: 12, marginBottom: 4 }}>信控额度</Text>
                <Text strong style={{ color: '#1890ff', fontSize: 18, fontFamily: 'monospace' }}>💳 {currencySymbol}{(rechargingUser?.credit_limit || 0).toFixed(6)}</Text>
              </div>
            )}
          </div>
        </div>

        <Form form={rechargeForm} layout="vertical" onFinish={handleRechargeSave} initialValues={{ actionType: 'increase', amount: '', walletType: 'system' }}>
          <Form.Item name="walletType" label={<Text strong>充值到哪个钱包</Text>} rules={[{ required: true }]}>
            <Radio.Group style={{ width: '100%', display: 'flex', flexWrap: 'wrap', gap: 8 }}>
              <Radio.Button value="system" style={{ borderRadius: 6 }}>系统钱包 (正常充值)</Radio.Button>
              <Radio.Button value="gift" style={{ borderRadius: 6 }}>赠送钱包 (活动赠送)</Radio.Button>
              <Radio.Button value="credit" style={{ borderRadius: 6 }}>💳 信控额度</Radio.Button>
            </Radio.Group>
          </Form.Item>
          
          <Form.Item name="actionType" label={<Text strong>操作类型</Text>} rules={[{ required: true }]}>
            <Radio.Group style={{ width: '100%', display: 'flex', gap: 8 }}>
              <Radio.Button value="increase" style={{ flex: 1, textAlign: 'center', borderRadius: 6 }}>
                增加金额 (+)
              </Radio.Button>
              <Radio.Button value="decrease" style={{ flex: 1, textAlign: 'center', borderRadius: 6 }}>
                减少金额 (-)
              </Radio.Button>
            </Radio.Group>
          </Form.Item>
          
          <Form.Item 
            name="amount" 
            label={<Text strong>{t('users.adjustment_amount')}</Text>} 
            rules={[
              { required: true, message: '请输入调整金额' },
              {
                validator: async (_, value) => {
                  if (value === undefined || value === null || value === '') {
                    return Promise.resolve();
                  }
                  const num = Number(value);
                  if (isNaN(num) || !isFinite(num)) {
                    return Promise.reject(new Error('请输入有效的数字金额'));
                  }
                  const reg = /^-?\d+(\.\d{1,6})?$/;
                  if (!reg.test(value.toString())) {
                    return Promise.reject(new Error('请输入正确的金额格式（最多保留六位小数，可为负数）'));
                  }
                  return Promise.resolve();
                }
              }
            ]}
          >
            <Input 
              style={{ width: '100%' }} 
              size="large"
              prefix={<span style={{ color: 'var(--ant-color-text-secondary)', marginRight: 4 }}>{currencySymbol}</span>}
              placeholder="0.000000"
            />
          </Form.Item>
          
          <div style={{ marginBottom: 20 }}>
            <Text type="secondary" style={{ fontSize: 13, marginBottom: 10, display: 'block' }}>
              快捷输入：<span style={{ color: '#1677ff', fontWeight: 500 }}>{
                rechargeWalletType === 'system' ? '系统钱包 (正常充值)' : 
                rechargeWalletType === 'gift' ? '赠送钱包 (活动赠送)' : '💳 信控额度'
              }</span>
            </Text>
            <div style={{ 
              display: 'grid', 
              gridTemplateColumns: 'repeat(4, 1fr)', 
              gap: 12 
            }}>
              {[10, 50, 100, 500, 1000, 10000, 20000, 100000].map(val => (
                <Button 
                  key={val} 
                  style={{ 
                    borderRadius: 8,
                    height: 36,
                    width: '100%',
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    fontWeight: 500,
                    color: rechargeActionType === 'decrease' ? '#ff4d4f' : '#52c41a', 
                    borderColor: rechargeActionType === 'decrease' ? 'rgba(255, 77, 79, 0.5)' : 'rgba(82, 196, 26, 0.5)',
                    background: rechargeActionType === 'decrease' ? 'rgba(255, 77, 79, 0.04)' : 'rgba(82, 196, 26, 0.04)'
                  }}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.borderColor = rechargeActionType === 'decrease' ? '#ff4d4f' : '#52c41a';
                    e.currentTarget.style.background = rechargeActionType === 'decrease' ? 'rgba(255, 77, 79, 0.1)' : 'rgba(82, 196, 26, 0.1)';
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.borderColor = rechargeActionType === 'decrease' ? 'rgba(255, 77, 79, 0.5)' : 'rgba(82, 196, 26, 0.5)';
                    e.currentTarget.style.background = rechargeActionType === 'decrease' ? 'rgba(255, 77, 79, 0.04)' : 'rgba(82, 196, 26, 0.04)';
                  }}
                  onClick={() => {
                    const cur = rechargeForm.getFieldValue('amount') || 0;
                    rechargeForm.setFieldsValue({ amount: Math.round((cur + val) * 1_000_000) / 1_000_000 });
                  }}
                >
                  {rechargeActionType === 'decrease' ? '-' : '+'}{val}
                </Button>
              ))}
            </div>
          </div>
          
          <Form.Item name="remark" label={<Text strong>{t('users.remark')}</Text>} style={{ marginBottom: 0 }}>
            <Input.TextArea rows={3} placeholder="输入调整备注信息 (必填/选填，建议填写以便后续对账)" style={{ borderRadius: 8 }} />
          </Form.Item>
        </Form>
      </Modal>

      {/* ── 钱包明细弹窗 ── */}
      <Modal
        title={`${walletDetailUser?.username || ''} 的钱包明细`}
        open={!!walletDetailUser}
        onCancel={() => { setWalletDetailUser(null); setWalletRecharges([]); }}
        footer={null}
        width={800}
        destroyOnClose
      >
        {walletDetailUser && (
          <WalletDetailsView 
            key={walletDetailUser.id}
            user={walletDetailUser} 
            recharges={walletRecharges} 
            loading={walletDetailLoading} 
          />
        )}
      </Modal>
    </Card>
  );
};

export default Users;

