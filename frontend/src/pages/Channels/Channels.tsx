/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia 
 * @license        MIT (https://www.tokensbyte.ai/)
 */

import React, { useEffect, useRef, useState } from 'react';
import ModelSelector from '../../components/ModelSelector';
import { Table, Button, Space, Tag, Modal, Form, Input, InputNumber, message, Popconfirm, Card, Typography, Select, Row, Col, Switch, Grid, Segmented, Tooltip, Divider, Alert, List, Progress, Drawer, Checkbox, Spin } from 'antd';
import MobileCardList, { MobileCard, CardRow, CardActions } from '../../components/MobileCardList';
import { PlusOutlined, EditOutlined, DeleteOutlined, SyncOutlined, ArrowLeftOutlined, ArrowRightOutlined, CloseOutlined, UnorderedListOutlined, AppstoreOutlined, PlayCircleOutlined, SearchOutlined, ApartmentOutlined, CloudServerOutlined, SettingOutlined, ThunderboltOutlined, ReloadOutlined, GlobalOutlined, ClearOutlined, StopOutlined, ExperimentOutlined, VideoCameraOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useNavigate, useLocation } from 'react-router-dom';
import request from '../../utils/request';
import useSettingsStore from '../../store/settings';
import type { Channel, ChannelCategory } from '../../types';
import { useThemeStore } from '../../store/theme';
import ChannelCategoryManager from '../../components/Channels/ChannelCategoryManager';
import {
  parseQuotaLimitInput,
  formatQuotaLimitDisplay,
  getEffectiveChannelPeriodUsed,
  validateQuotaHierarchy,
  isFiniteQuotaLimit,
} from '../../utils/quotaPeriod';

// ── 插件动态加载（各插件均可独立移除，删除对应目录后自动降级，不影响主功能） ──
type HHPluginModule = any;


const { Title, Text } = Typography;
const { Option } = Select;
const { useBreakpoint } = Grid;

/** 视频/图片常用分辨率档（优先按模型类型，其次渠道分类） */
const VIDEO_RES_PRESETS = ['480p', '720p', '1080p', '2k', '4k'] as const;
const IMAGE_RES_PRESETS = ['1k', '2k', '4k'] as const;

/** 与后端 normalize_resolution_label 对齐，保证存库 key 能命中请求分辨率 */
function normalizeResKey(raw: string): string {
  let res = raw.trim().toLowerCase().replace(/\*/g, 'x');
  if (res && /^[0-9]+$/.test(res)) res += 'p';
  return res;
}

type ResModelMapping = Record<string, Record<string, Record<string, string>>>;

const isFilled = (v: unknown) => !!v && String(v).trim().length > 0;
const countFilled = (obj?: Record<string, unknown> | null) =>
  Object.values(obj || {}).filter(isFilled).length;

const COMFYUI_DISPATCH_DEFAULT = 'priority_weight';

function parseComfyuiServerIds(
  cfg: Record<string, any> | undefined,
  workflows: { id: number; server_id?: number }[] = [],
): number[] {
  const raw = Array.isArray(cfg?.comfyui_server_ids) ? cfg!.comfyui_server_ids : [];
  const ids: number[] = [];
  for (const v of raw) {
    const id = Number(v);
    if (id > 0 && !ids.includes(id)) ids.push(id);
  }
  if (ids.length) return ids;
  const sid = cfg?.comfyui_server_id
    ?? workflows.find((w) => w.id === cfg?.comfyui_workflow_id)?.server_id;
  const n = Number(sid);
  return n > 0 ? [n] : [];
}

/** 按 valid 剪枝 Record 顶层 key；无变化时返回原引用 */
function pruneRecordKeys<T>(prev: Record<string, T>, valid: Set<string>): Record<string, T> {
  let changed = false;
  const next = { ...prev };
  for (const k of Object.keys(next)) {
    if (!valid.has(k)) {
      delete next[k];
      changed = true;
    }
  }
  return changed ? next : prev;
}

/** 设置/删除某模型某 scope 的分辨率映射表 */
function patchResScope(
  prev: ResModelMapping,
  modelId: string,
  scope: string,
  map: Record<string, string> | null,
): ResModelMapping {
  const scopes = { ...(prev[modelId] || {}) };
  if (!map || Object.keys(map).length === 0) delete scopes[scope];
  else scopes[scope] = map;
  if (Object.keys(scopes).length === 0) {
    const next = { ...prev };
    delete next[modelId];
    return next;
  }
  return { ...prev, [modelId]: scopes };
}

/** 读入/保存共用：规范化分辨率 key、去掉空值；可选只保留仍绑定的子渠 */
function cleanResModelMapping(
  raw: ResModelMapping | Record<string, any> | null | undefined,
  allowedSubIds?: number[],
): ResModelMapping {
  const cleaned: ResModelMapping = {};
  for (const [modelId, scopes] of Object.entries(raw || {})) {
    const validScopes: Record<string, Record<string, string>> = {};
    for (const [scope, resMap] of Object.entries((scopes || {}) as Record<string, Record<string, string>>)) {
      if (scope !== 'default' && allowedSubIds && !allowedSubIds.includes(Number(scope))) continue;
      const validRes: Record<string, string> = {};
      for (const [res, alias] of Object.entries(resMap || {})) {
        const key = normalizeResKey(String(res || ''));
        if (key && isFilled(alias)) validRes[key] = String(alias).trim();
      }
      if (Object.keys(validRes).length > 0) validScopes[scope] = validRes;
    }
    if (Object.keys(validScopes).length > 0) cleaned[modelId] = validScopes;
  }
  return cleaned;
}

/** 按模型类型优先选分辨率档（其次名称/id，最后渠道分类）；图片→1k…，视频→480p… */
function resPresetsForHint(...hints: Array<string | null | undefined>): string[] {
  for (const h of hints) {
    const n = (h || '').toLowerCase();
    if (!n) continue;
    if (/视|video|seedance|kling|runway|luma|minimax.?video/.test(n)) return [...VIDEO_RES_PRESETS];
    if (/图|image|dall|flux|midjourney|sdxl|gpt-image|imagen/.test(n)) return [...IMAGE_RES_PRESETS];
  }
  return [...IMAGE_RES_PRESETS];
}

function resScopeKey(modelId: string, scope: string) {
  return `${modelId}:${scope}`;
}

const Channels: React.FC = () => {
  const { themeMode } = useThemeStore();
  const isLight = themeMode === 'light';
  const { t } = useTranslation();
  const { settings } = useSettingsStore();
  const currencySymbol = settings?.currency?.currency_symbol || '$';
  const adminPath = settings?.site?.admin_path || 'admin1688';
  const quotaTz = settings?.site?.default_timezone || 'Asia/Shanghai';
  const [channels, setChannels] = useState<Channel[]>([]);
  const [viewMode, setViewMode] = useState<'list' | 'card'>(() => {
    return (localStorage.getItem('channels_view_mode') as 'list' | 'card') || 'card';
  });
  const [availableModels, setAvailableModels] = useState<any[]>([]);
  const [availableUserLevels, setAvailableUserLevels] = useState<any[]>([]);
  const [presets, setPresets] = useState<any[]>([]);
  /** 凭证 id → 启用中的素材绑定；分组页靠它标识「本分组会承担素材上游」（开关打开后） */
  const [assetBindingByPreset, setAssetBindingByPreset] = useState<
    Record<number, { id: number; name: string; count: number }>
  >({});
  const [activePlugins, setActivePlugins] = useState<Record<string, boolean>>({});
  const [loading, setLoading] = useState(true);
  const [isModalVisible, setIsModalVisible] = useState(() => {
    return new URLSearchParams(window.location.search).has('edit');
  });
  const [editingChannel, setEditingChannel] = useState<Channel | null>(null);
  const [form] = Form.useForm();
  const navigate = useNavigate();
  const location = useLocation();

  const handleCloseModal = () => {
    setIsModalVisible(false);
    if (location.state && (location.state as any).from === 'model-display') {
      navigate(`/${adminPath}/channels/model-display`);
    }
  };
  const [showMapping, setShowMapping] = useState(false);
  const [activeMappingInputs, setActiveMappingInputs] = useState<string[]>([]);
  const [modelMappingState, setModelMappingState] = useState<Record<string, string>>({});
  // 高可用渠道组：按子渠道独立别名映射 { model_id: { sub_channel_id: alias } }
  const [haModelMappingState, setHaModelMappingState] = useState<Record<string, Record<string, string>>>({});
  // 分辨率映射：{ model_id: { "default"|subId: { "480p"|"1k": alias } } }
  const [resModelMappingState, setResModelMappingState] = useState<ResModelMapping>({});
  const [expandedHaModels, setExpandedHaModels] = useState<string[]>([]);
  // 展开的分辨率高级面板 key：`${modelId}:${scopeKey}`
  const [expandedResScopes, setExpandedResScopes] = useState<string[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const screens = useBreakpoint();
  const [isExcludeMode, setIsExcludeMode] = useState(false);
  const [activeRightPanel, setActiveRightPanel] = useState<'models' | 'levels' | 'mapping' | 'presets'>('models');
  const [presetSearchText, setPresetSearchText] = useState('');
  /** 选择上游渠道 / 高可用绑定物理上游：全部 | 激活 | 禁用 */
  const [upstreamStatusFilter, setUpstreamStatusFilter] = useState<'all' | 1 | 0>('all');
  const [enableQuota, setEnableQuota] = useState(false);
  const [upstreamTab, setUpstreamTab] = useState<'preset' | 'volcengine_enhance' | 'comfyui'>('preset');
  const [volcengineEnhanceKeys, setVolcengineEnhanceKeys] = useState<any[]>([]);
  const [comfyuiWorkflows, setComfyuiWorkflows] = useState<any[]>([]);
  const [comfyuiServers, setComfyuiServers] = useState<any[]>([]);
  const [comfyuiDispatchRules, setComfyuiDispatchRules] = useState<any[]>([]);
  /** 模型选择的桥接状态，同步 form store 与 ModelSelector 双向数据 */
  const [channelModelMids, setChannelModelMids] = useState<string[]>([]);
  const [selectedSubChannelAids, setSelectedSubChannelAids] = useState<any[]>([]);
  const [haMaxRetries, setHaMaxRetries] = useState<number>(3);
  /** 组内绑定级 P/W：{ "<channel_configs.id>": { priority?, weight? } }；空对象 = 未物化，整组跟随上游全局值 */
  const [subChannelPolicyState, setSubChannelPolicyState] = useState<Record<string, { priority?: number; weight?: number }>>({});

  // ===== 需求一：组内上游优先级 / 权重 =====
  // 生效值口径与后端 relay::ha::parse_sub_channel_policy 一致：组内策略 ?? 上游全局值
  const subPolicyMaterialized = Object.keys(subChannelPolicyState).length > 0;
  const effSubPriority = (p: any) => subChannelPolicyState[String(p.id)]?.priority ?? p.priority ?? 0;
  const effSubWeight = (p: any) => subChannelPolicyState[String(p.id)]?.weight ?? p.weight ?? 1;
  const isSubPolicyFieldSet = (id: any, field: 'priority' | 'weight') =>
    subChannelPolicyState[String(id)]?.[field] !== undefined;
  const subPolicyValue = (id: any, field: 'priority' | 'weight') =>
    (isSubPolicyFieldSet(id, field) ? subChannelPolicyState[String(id)]?.[field] : undefined) as number | undefined;
  /** 实际改动组内 P/W 才物化：先把该组全部已绑上游的当前生效值写满，再覆盖被改的那一行 */
  const setSubPolicyField = (configId: any, field: 'priority' | 'weight', value: number | null) => {
    setSubChannelPolicyState(prev => {
      let base = prev;
      if (Object.keys(prev).length === 0) {
        base = {};
        selectedSubChannelAids.forEach((aid: any) => {
          const preset = presets.find(p => p.id === aid);
          if (!preset) return;
          base[String(aid)] = { priority: preset.priority ?? 0, weight: preset.weight ?? 1 };
        });
      }
      const cur = { ...(base[String(configId)] || {}) };
      if (value === null || value === undefined) delete cur[field];
      else cur[field] = Math.round(value);
      return { ...base, [String(configId)]: cur };
    });
  };
  /** 绑定变化：已物化时新绑的按其全局值入册、解绑的立即剔除；未物化保持空 */
  const syncSubPolicyOnBind = (nextIds: any[]) => {
    setSubChannelPolicyState(prev => {
      if (Object.keys(prev).length === 0) return prev;
      const keep: Record<string, { priority?: number; weight?: number }> = {};
      nextIds.forEach((aid: any) => {
        const key = String(aid);
        if (prev[key]) { keep[key] = prev[key]; return; }
        const preset = presets.find(p => p.id === aid);
        if (preset) keep[key] = { priority: preset.priority ?? 0, weight: preset.weight ?? 1 };
      });
      return keep;
    });
  };
  // 熔断状态 Map: { channelId: { channel_meltdown, sub_channels } }
  const [meltdownMap, setMeltdownMap] = useState<Record<number, any>>({});
  const [meltdownLoading, setMeltdownLoading] = useState<Record<number, boolean>>({});

  // useRef to hold reliable copies of models/levels outside AntD form store
  // (form store gets corrupted when model_mapping Form.Items are registered)
  const modelsRef = useRef<string[]>([]);
  const levelsRef = useRef<string[]>([]);
  // Plugin: happyhorse_router 状态（插件可移除，移除后这些状态保持默认值，所有条件渲染自动跳过）
  const [isHappyHorseRouting, setIsHappyHorseRouting] = useState<boolean>(false);
  const [happyHorseConfigs, setHappyHorseConfigs] = useState<any[]>([]);
  const [selectedRoutingNode, setSelectedRoutingNode] = useState<string | null>(null);
  const [happyHorseEnabled, setHappyHorseEnabled] = useState<boolean>(false);
  const [hhModule, setHhModule] = useState<HHPluginModule | null>(null);

  const [statusFilter, setStatusFilter] = useState<number | 'all'>(1);
  const [categoryFilter, setCategoryFilter] = useState<number | 'all' | 'unclassified'>('all');
  const [typeFilter, setTypeFilter] = useState<'all' | 'default' | 'volcengine' | 'ha' | 'comfyui'>('all');
  const [searchQuery, setSearchQuery] = useState('');
  const [configObj, setConfigObj] = useState<Record<string, any>>({});
  const [categories, setCategories] = useState<ChannelCategory[]>([]);
  const [isCategoryManagerVisible, setIsCategoryManagerVisible] = useState(false);

  // 清除画质增强凭证关联（取消选择时复用）
  const clearVolcengineEnhance = () => {
    setUpstreamTab('preset');
    form.setFieldsValue({ provider_type: 'custom' });
    setConfigObj(prev => {
      const { volcengine_enhance_credential_id, ...rest } = prev;
      return rest;
    });
  };

  const clearComfyui = () => {
    setUpstreamTab('preset');
    form.setFieldsValue({ provider_type: 'custom' });
    setConfigObj(prev => {
      const { comfyui_workflow_id, comfyui_server_id, comfyui_server_ids, comfyui_dispatch, ...rest } = prev;
      return rest;
    });
  };

  const getChannelTypeKey = (c: Channel): 'default' | 'volcengine' | 'ha' | 'comfyui' => {
    if (c.provider_type === 'high_availability_group') return 'ha';
    if (c.provider_type === 'volcengine') return 'volcengine';
    if (c.provider_type === 'comfyui') return 'comfyui';
    return 'default';
  };

  const filteredChannels = channels.filter(c => {
    let matchStatus = true;
    if (statusFilter !== 'all') {
      matchStatus = c.status === statusFilter;
    }
    
    let matchSearch = true;
    if (searchQuery) {
      const q = searchQuery.toLowerCase();
      matchSearch = !!((c.name && c.name.toLowerCase().includes(q)) || 
                    (c.group_aid && c.group_aid.toLowerCase().includes(q)));
    }

    let matchCategory = true;
    if (categoryFilter !== 'all') {
      if (categoryFilter === 'unclassified') {
        matchCategory = !c.category_id;
      } else {
        matchCategory = c.category_id === categoryFilter;
      }
    }

    let matchType = true;
    if (typeFilter !== 'all') {
      matchType = getChannelTypeKey(c) === typeFilter;
    }

    return matchStatus && matchSearch && matchCategory && matchType;
  });

  const fetchChannels = async () => {
    setLoading(true);
    try {
      const resp = await (request.get('/channels') as unknown as Promise<{ data: Channel[] }>);
      setChannels(resp.data || []);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  };

  const fetchModels = async () => {
    try {
      const resp = await (request.get('/models') as unknown as Promise<{ data: any[] }>);
      setAvailableModels(resp.data || []);
    } catch (e) {
      console.error(e);
    }
  };

  const fetchUserLevels = async () => {
    try {
      const resp = await (request.get('/user_levels') as unknown as Promise<{ data: any[] }>);
      setAvailableUserLevels(resp.data || []);
    } catch (e) {
      console.error(e);
    }
  };

  const fetchHaMaxRetries = async () => {
    try {
      const res = await (request.get('/plugins/high_availability_channel/ha-config') as Promise<any>);
      if (res && res.ha_max_retries) {
        setHaMaxRetries(res.ha_max_retries);
      }
    } catch (e) {
      console.error('加载高可用插件配置失败:', e);
    }
  };

  const fetchPresets = async () => {
    try {
      const resp = await (request.get('/channel-configs') as unknown as Promise<{ data: any[] }>);
      const data = resp.data || [];
      const haPreset = {
        id: -99,
        name: '高可用虚拟渠道组',
        provider_type: 'high_availability_group',
        yid: 'HA',
        rate: 1.0,
      };
      setPresets([haPreset, ...data]);
    } catch (e) {
      console.error(e);
    }
  };

  /** 拉一次启用中的素材绑定并按凭证归组：同凭证多条属异常配置，取最小 id 但保留条数以显式告警 */
  const fetchAssetBindings = async () => {
    try {
      const resp = await (request.get('/upstream-asset-bindings') as unknown as Promise<{ data: any[] }>);
      const byCfg = new Map<number, any[]>();
      (resp.data || [])
        .filter((b) => b.is_active === 1)
        .forEach((b) => {
          const k = Number(b.channel_config_id);
          const arr = byCfg.get(k) || [];
          arr.push(b);
          byCfg.set(k, arr);
        });
      const map: Record<number, { id: number; name: string; count: number }> = {};
      byCfg.forEach((arr, k) => {
        arr.sort((a, b) => a.id - b.id);
        map[k] = { id: arr[0].id, name: arr[0].name, count: arr.length };
      });
      setAssetBindingByPreset(map);
    } catch (e) {
      console.error('加载上游素材绑定失败:', e);
    }
  };

  const fetchCategories = async () => {
    try {
      const resp = await (request.get('/channel-categories') as any);
      setCategories(Array.isArray(resp) ? resp : []);
    } catch (e) {
      console.error(e);
    }
  };

  const resolveCategoryName = (categoryId?: number | null) => {
    if (!categoryId) return null;
    const cat = categories.find(c => c.id === categoryId);
    return cat?.name || null;
  };

  const activeCategories = categories.filter(c => !!c.is_active);

  /** 上游列的小尾巴：该凭证上挂着启用中的素材绑定就标出来，避免运营在分组页与绑定页之间对不上号 */
  const renderAssetBindingTag = (presetId?: number | null, style?: React.CSSProperties) => {
    if (!presetId) return null;
    const ab = assetBindingByPreset[presetId];
    if (!ab) return null;
    return (
      <Tooltip
        title={ab.count > 1
          ? `该凭证上有 ${ab.count} 条启用中的素材绑定（异常配置），此处按最小 id 显示为 #${ab.id}「${ab.name}」；请只保留一条启用绑定`
          : `打开「按渠道分组推导素材上游」后，本分组承担素材上游：绑定#${ab.id}「${ab.name}」`}
      >
        <Tag color={ab.count > 1 ? 'warning' : 'gold'} style={style}>
          素材{ab.count > 1 ? `×${ab.count}` : ''}
        </Tag>
      </Tooltip>
    );
  };

  const fetchPluginsAndPools = async () => {
    try {
      const resp = await (request.get('/plugins') as unknown as Promise<{ plugins: any[] }>);
      const plugins = resp.plugins || [];
      const activeMap: Record<string, boolean> = {};
      let hasVolcengineEnhance = false;
      let hasComfyui = false;

      plugins.forEach(p => {
        if (p.is_enabled === 1) {
          activeMap[p.name] = true;
          if (p.name === 'volcengine_enhance') hasVolcengineEnhance = true;
          if (p.name === 'comfyui_bridge') hasComfyui = true;
          // Plugin: happyhorse_router 数据加载（插件可移除）
          if (p.name === 'happyhorse_router') {
            setHappyHorseEnabled(true);
            request.get('/plugins/happyhorse_router/configs').then((r: any) => {
              const data = r?.data ?? r;
              const list = data?.configs ?? [];
              setHappyHorseConfigs(list);
            }).catch(() => {});
            // 动态加载插件 UI 模块
            const modGlob = import.meta.glob('../Plugins/HappyHorse/HappyHorseChannelPlugin.tsx');
            if (modGlob['../Plugins/HappyHorse/HappyHorseChannelPlugin.tsx']) {
              modGlob['../Plugins/HappyHorse/HappyHorseChannelPlugin.tsx']().then((mod: any) => setHhModule(mod)).catch(() => {});
            }
            }
          }
      });
      setActivePlugins(activeMap);

      if (hasVolcengineEnhance) {
        request.get('/plugins/volcengine_enhance/volcengine-enhance-config').then((r: any) => {
          if (r && r.keys) {
            setVolcengineEnhanceKeys(r.keys);
          }
        }).catch(() => {});
      }
      if (hasComfyui) {
        request.get('/plugins/comfyui_bridge/servers').then((r: any) => {
          setComfyuiServers(r?.servers || []);
        }).catch(() => {});
        request.get('/plugins/comfyui_bridge/workflows').then((r: any) => {
          setComfyuiWorkflows(r?.workflows || []);
        }).catch(() => {});
        request.get('/plugins/comfyui_bridge/dispatch-rules').then((r: any) => {
          setComfyuiDispatchRules(r?.rules || []);
        }).catch(() => {});
      }
    } catch (e) {
      console.error(e);
    }
  };


  // 批量加载所有 HA 渠道的熔断状态
  const fetchAllMeltdownStatus = async (channelList?: Channel[]) => {
    const list = channelList || channels;
    const haChannels = list.filter(c => c.provider_type === 'high_availability_group');
    if (haChannels.length === 0) return;
    const results: Record<number, any> = {};
    await Promise.allSettled(
      haChannels.map(async (ch) => {
        try {
          const resp = await request.get(`/channels/${ch.id}/meltdown`) as any;
          results[ch.id] = resp;
        } catch { /* ignore */ }
      })
    );
    setMeltdownMap(prev => ({ ...prev, ...results }));
  };

  // 手动重置单个渠道的熔断
  const handleResetMeltdown = async (channelId: number) => {
    setMeltdownLoading(prev => ({ ...prev, [channelId]: true }));
    try {
      const resp = await request.post(`/channels/${channelId}/meltdown/reset`) as any;
      message.success(`已重置熔断状态，清除了 ${resp.cleared_count || 0} 条记录`);
      // 刷新该渠道的熔断状态
      try {
        const updated = await request.get(`/channels/${channelId}/meltdown`) as any;
        setMeltdownMap(prev => ({ ...prev, [channelId]: updated }));
      } catch { /* ignore */ }
    } catch {
      message.error('重置熔断状态失败');
    } finally {
      setMeltdownLoading(prev => ({ ...prev, [channelId]: false }));
    }
  };

  // 手动清零渠道已用额度（总/日/月）
  const handleResetQuota = async (channelId: number) => {
    try {
      await request.post(`/channels/${channelId}/quota/reset`);
      message.success('已清零渠道已用额度');
      fetchChannels();
    } catch {
      message.error('清零额度失败');
    }
  };

  useEffect(() => {
    fetchChannels();
    fetchModels();
    fetchUserLevels();
    fetchPresets();
    fetchAssetBindings();
    fetchCategories();
    fetchPluginsAndPools();
    fetchHaMaxRetries();
  }, []);

  // 渠道列表加载后自动获取 HA 渠道熔断状态
  useEffect(() => {
    if (channels.length > 0) {
      fetchAllMeltdownStatus(channels);
    }
  }, [channels]);

  // 选中的渠道类型在列表中已不存在时，回退到「全部」
  useEffect(() => {
    if (typeFilter === 'all') return;
    const stillExists = channels.some((ch) => getChannelTypeKey(ch) === typeFilter);
    if (!stillExists) setTypeFilter('all');
  }, [channels, typeFilter]);

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const editId = params.get('edit');
    if (editId && channels.length > 0 && availableModels.length > 0) {
      const channelId = parseInt(editId);
      const ch = channels.find(c => c.id === channelId);
      if (ch) {
        window.history.replaceState(null, '', window.location.pathname);
        handleEdit(ch);
      }
    }
  }, [channels, availableModels]);

  const handleAdd = () => {
    setEditingChannel(null);
    setEnableQuota(false);
    form.resetFields();
    setShowMapping(false);
    setActiveMappingInputs([]);
    setModelMappingState({});
    setHaModelMappingState({});
    setResModelMappingState({});
    setExpandedHaModels([]);
    setExpandedResScopes([]);
    setIsExcludeMode(false);
    setChannelModelMids([]);
    setActiveRightPanel('models');
    setPresetSearchText('');
    setUpstreamStatusFilter('all');
    setUpstreamTab('preset');
    setConfigObj({});
    setSelectedSubChannelAids([]);
    modelsRef.current = [];
    levelsRef.current = [];
    setIsHappyHorseRouting(false);
    setSelectedRoutingNode(null);
    setIsModalVisible(true);
  };

  const handleEdit = (record: Channel) => {
    setEditingChannel(record);
    let mapping: Record<string, string> = {};
    try {
      mapping = typeof record.model_mapping === 'string' ? JSON.parse(record.model_mapping) : (record.model_mapping || {});
    } catch (e) {}

    // models 兼容处理：新格式存 mid，旧格式存 model_id，需要统一转为 mid
    const rawModels = Array.isArray(record.models) ? record.models : [];
    const modelsForForm = rawModels.map((val: string) => {
      if (availableModels.find(m => m.mid === val)) return val;
      const match = availableModels.find(m => m.model_id === val);
      return match ? match.mid : val;
    });

    const levelIds = (record.exclude_user_groups && record.exclude_user_groups.length > 0)
      ? record.exclude_user_groups
      : (record.user_groups || []);

    // Sync refs (reliable source of truth for save)
    modelsRef.current = modelsForForm;
    levelsRef.current = levelIds;
    setChannelModelMids(modelsForForm);

    const hasHappyHorse = rawModels.some((m: string) => m.startsWith('ephh-'));
    setIsHappyHorseRouting(hasHappyHorse);
    if (hasHappyHorse) {
      const node = rawModels.find((m: string) => m.startsWith('ephh-')) || '';
      setSelectedRoutingNode(node);
    } else {
      setSelectedRoutingNode(null);
    }

    const hasMapping = Object.values(mapping).some(v => v && String(v).trim());
    setShowMapping(hasMapping);
    const q = record.quota_limit ?? -1;
    const dq = record.daily_quota_limit ?? -1;
    const wq = record.weekly_quota_limit ?? -1;
    const mq = record.monthly_quota_limit ?? -1;
    setEnableQuota(q >= 0 || dq >= 0 || wq >= 0 || mq >= 0);
    setModelMappingState(mapping);
    setActiveMappingInputs(Object.keys(mapping).filter(k => mapping[k] && String(mapping[k]).trim()));
    setIsExcludeMode(!!record.exclude_user_groups && record.exclude_user_groups.length > 0);
    setActiveRightPanel('models');
    setPresetSearchText('');
    setUpstreamStatusFilter('all');
    if (record.preset_id) {
      setUpstreamTab('preset');
    } else if (record.provider_type === 'volcengine') {
      setUpstreamTab('volcengine_enhance');
    } else if (record.provider_type === 'comfyui') {
      setUpstreamTab('comfyui');
    } else {
      setUpstreamTab('preset');
    }


    // 解析 config JSON 初始化存储设置状态
    let parsedConfig: Record<string, any> = {};
    try {
      parsedConfig = record.config
        ? (typeof record.config === 'string' ? JSON.parse(record.config) : record.config)
        : {};
    } catch { parsedConfig = {}; }
    setConfigObj(parsedConfig);
    const subAids = parsedConfig.sub_channels || [];
    setSelectedSubChannelAids(subAids);
    // 组内 P/W：config 无 sub_channel_policy 键 = 未物化，整组跟随上游全局值
    const rawSubPolicy = parsedConfig?.sub_channel_policy;
    const initSubPolicy: Record<string, { priority?: number; weight?: number }> = {};
    if (rawSubPolicy && typeof rawSubPolicy === 'object' && !Array.isArray(rawSubPolicy)) {
      Object.entries(rawSubPolicy).forEach(([k, v]: [string, any]) => {
        const entry: { priority?: number; weight?: number } = {};
        if (Number.isFinite(v?.priority)) entry.priority = Math.round(v.priority);
        if (Number.isFinite(v?.weight)) entry.weight = Math.round(v.weight);
        if (Object.keys(entry).length > 0) initSubPolicy[k] = entry;
      });
    }
    setSubChannelPolicyState(initSubPolicy);

    // 恢复高可用子渠道独立映射（必须在 parsedConfig 解析之后）
    const haMapping = parsedConfig?.ha_model_mapping || {};
    setHaModelMappingState(haMapping);
    // 恢复分辨率映射（读入时规范化 key，与后端命中规则一致）
    const resMapping = cleanResModelMapping(parsedConfig?.res_model_mapping);
    setResModelMappingState(resMapping);
    // 自动展开：有明文 HA 映射或分辨率映射的模型 + 已配置的分辨率高级面板
    const resExpanded = Object.entries(resMapping).flatMap(([mid, scopes]) =>
      Object.entries(scopes || {})
        .filter(([, map]) => countFilled(map) > 0)
        .map(([scope]) => resScopeKey(mid, scope)),
    );
    setExpandedResScopes(resExpanded);
    setExpandedHaModels(Array.from(new Set([
      ...Object.keys(haMapping).filter(k => countFilled(haMapping[k]) > 0),
      ...Object.keys(resMapping).filter(mid =>
        Object.values(resMapping[mid] || {}).some(m => countFilled(m) > 0),
      ),
    ])));
    if (resExpanded.length > 0 || Object.values(haMapping).some(sub => countFilled(sub as Record<string, unknown>) > 0)) {
      setShowMapping(true);
    }

    setIsModalVisible(true);

    setTimeout(() => {
      form.setFieldsValue({
        name: record.name,
        provider_type: record.provider_type || 'custom',
        base_url: record.base_url,
        api_key: (record as any).api_key || '',
        sort_order: record.sort_order || 0,
        category_id: record.category_id || null,
        priority: record.priority || 0,
        status: record.status ?? 1,
        weight: record.weight || 1,
        rate: record.rate ?? 1.0,
        max_rps: (record as any).max_rps || 0,
        quota_limit: record.quota_limit ?? -1,
        quota_used: record.quota_used || 0,
        daily_quota_limit: record.daily_quota_limit ?? -1,
        weekly_quota_limit: record.weekly_quota_limit ?? -1,
        monthly_quota_limit: record.monthly_quota_limit ?? -1,
        preset_id: record.preset_id || null,
        model_mapping: mapping,
        models: modelsForForm,
        level_select: levelIds,
      });
    }, 0);
  };

  const handleDelete = async (id: number) => {
    try {
      await request.delete(`/channels/${id}`);
      message.success(t('common.success'));
      fetchChannels();
    } catch (e) {
      console.error(e);
    }
  };

  const handleToggleStatus = async (record: Channel) => {
    try {
      const newStatus = record.status === 1 ? 0 : 1;
      if (newStatus === 1 && record.provider_type === 'high_availability_group' && !activePlugins['high_availability_channel']) {
        message.warning('高可用上游渠道系统插件未开启，无法启用此渠道');
        return;
      }
      await request.put(`/channels/${record.id}`, { status: newStatus });
      setChannels(prev => prev.map(c => c.id === record.id ? { ...c, status: newStatus } : c));
      message.success(newStatus === 1 ? '已启用渠道' : '已禁用渠道');
    } catch (e) {
      console.error(e);
      message.error('状态更新失败');
    }
  };

  const handleTest = (record: Channel) => {
    navigate(`/${adminPath}/channels/test/${record.id}`);
  };

  const handleModelsChange = (nextModels: string[]) => {
    form.setFieldsValue({ models: nextModels });
    modelsRef.current = nextModels;
    setChannelModelMids(nextModels);

    const currentMapping = modelMappingState;
    const validModelIds = new Set(nextModels.map(mid => {
      const match = availableModels.find(m => m.mid === mid);
      return match ? match.model_id : mid;
    }));

    let mappingChanged = false;
    const newMapping = { ...currentMapping };
    for (const key of Object.keys(newMapping)) {
      if (!validModelIds.has(key)) {
        delete newMapping[key];
        mappingChanged = true;
      }
    }

    if (mappingChanged) {
      setModelMappingState(newMapping);
      form.setFieldsValue({ model_mapping: newMapping });
      setActiveMappingInputs(prev => prev.filter(id => validModelIds.has(id)));
    }

    // 同步清理 HA / 分辨率映射中已移除的模型
    setHaModelMappingState(prev => pruneRecordKeys(prev, validModelIds));
    setResModelMappingState(prev => pruneRecordKeys(prev, validModelIds));
    setExpandedHaModels(prev => prev.filter(id => validModelIds.has(id)));
    setExpandedResScopes(prev => prev.filter(k => validModelIds.has(k.split(':')[0])));
  };

  const handleSave = async (values: any) => {
    if (submitting) return;
    setSubmitting(true);
    const finalMapping: Record<string, string> = {};
    const currentModelMapping = modelMappingState;
    if (showMapping && currentModelMapping) {
      for (const [k, v] of Object.entries(currentModelMapping)) {
        if (v && String(v).trim()) {
          finalMapping[k] = String(v).trim();
        }
      }
    }


    // Read models and levels from refs (immune to form store corruption)
    const reliableModels = modelsRef.current;
    const reliableLevels = levelsRef.current;

    if (reliableModels.length === 0) {
      message.error('请选择至少一个模型');
      setSubmitting(false);
      return;
    }

    const isHaGroup = values.provider_type === 'high_availability_group';
    if (isHaGroup && selectedSubChannelAids.length === 0) {
      message.error('高可用虚拟组必须至少绑定一个子渠道');
      setSubmitting(false);
      return;
    }

    if (!isHaGroup && values.provider_type !== 'volcengine' && values.provider_type !== 'comfyui' && !values.preset_id) {
      message.error('请选择上游渠道（预设、卡池或插件上游至少选其一）');
      setSubmitting(false);
      return;
    }

    // 画质增强渠道必须关联凭证
    if (values.provider_type === 'volcengine' && !configObj.volcengine_enhance_credential_id) {
      message.error('请选择画质增强凭证');
      setSubmitting(false);
      return;
    }

    const comfyServerIds = parseComfyuiServerIds(configObj, comfyuiWorkflows);
    if (values.provider_type === 'comfyui' && comfyServerIds.length === 0) {
      message.error('请至少选择一个 ComfyUI 服务节点');
      setSubmitting(false);
      return;
    }
    const comfyServerId = comfyServerIds[0];
    const comfyLegacyWf = comfyuiWorkflows.find((w) => w.id === configObj.comfyui_workflow_id);
    const wfNodeIds: number[] = Array.isArray(comfyLegacyWf?.server_ids)
      ? comfyLegacyWf.server_ids.map(Number).filter((id: number) => id > 0)
      : (comfyLegacyWf?.server_id ? [Number(comfyLegacyWf.server_id)] : []);
    const keepComfyWorkflowId = comfyLegacyWf && wfNodeIds.some((id: number) => comfyServerIds.includes(id))
      ? comfyLegacyWf.id
      : undefined;

    // Ensure only one upstream is used and others are explicitly cleared
    let { preset_id } = values;
    if (isHaGroup) {
      preset_id = null;
    }

    // 构建 HA 子渠道独立映射（只保留有实际值的条目）
    let finalHaModelMapping: Record<string, Record<string, string>> | undefined;
    if (isHaGroup && showMapping) {
      const cleaned: Record<string, Record<string, string>> = {};
      for (const [modelId, subMap] of Object.entries(haModelMappingState)) {
        const validEntries: Record<string, string> = {};
        for (const [subId, alias] of Object.entries(subMap)) {
          if (alias && String(alias).trim() && selectedSubChannelAids.includes(Number(subId))) {
            validEntries[subId] = String(alias).trim();
          }
        }
        if (Object.keys(validEntries).length > 0) {
          cleaned[modelId] = validEntries;
        }
      }
      if (Object.keys(cleaned).length > 0) {
        finalHaModelMapping = cleaned;
      }
    }

    // 构建组内 P/W 策略（未物化则整组不写键；已物化则只保留仍绑定的条目）
    let finalSubChannelPolicy: Record<string, { priority?: number; weight?: number }> | undefined;
    if (isHaGroup && subPolicyMaterialized) {
      const boundIds = selectedSubChannelAids.map((id: any) => Number(id));
      const cleaned: Record<string, { priority?: number; weight?: number }> = {};
      for (const [subId, item] of Object.entries(subChannelPolicyState)) {
        if (!boundIds.includes(Number(subId))) continue;
        const entry: { priority?: number; weight?: number } = {};
        if (typeof item?.priority === 'number') entry.priority = item.priority;
        if (typeof item?.weight === 'number') entry.weight = item.weight;
        if (Object.keys(entry).length > 0) cleaned[subId] = entry;
      }
      if (Object.keys(cleaned).length > 0) finalSubChannelPolicy = cleaned;
    }

    // 构建分辨率映射（默认 + 仍绑定的子渠；只保留非空档位）
    let finalResModelMapping: ResModelMapping | undefined;
    if (showMapping) {
      // HA：过滤已解绑子渠；非 HA：传 [] 仅保留 default
      const cleaned = cleanResModelMapping(
        resModelMappingState,
        isHaGroup ? selectedSubChannelAids : [],
      );
      if (Object.keys(cleaned).length > 0) {
        finalResModelMapping = cleaned;
      }
    }

    const finalConfig = isHaGroup 
      ? {
          ...configObj,
          sub_channels: selectedSubChannelAids,
          ...(finalSubChannelPolicy ? { sub_channel_policy: finalSubChannelPolicy } : { sub_channel_policy: undefined }),
          ...(finalHaModelMapping ? { ha_model_mapping: finalHaModelMapping } : { ha_model_mapping: undefined }),
          ...(finalResModelMapping ? { res_model_mapping: finalResModelMapping } : { res_model_mapping: undefined }),
        }
      : {
          tos_storage_enabled: configObj.tos_storage_enabled,
          tos_storage_days: configObj.tos_storage_days,
          // 画质增强凭证关联：通过凭证 ID 实时查询最新密钥，保证数据一致性
          ...(values.provider_type === 'volcengine' && configObj.volcengine_enhance_credential_id
            ? { volcengine_enhance_credential_id: configObj.volcengine_enhance_credential_id }
            : {}),
          ...(values.provider_type === 'comfyui' && comfyServerId
            ? {
                comfyui_server_id: comfyServerId,
                comfyui_server_ids: comfyServerIds,
                comfyui_dispatch: configObj.comfyui_dispatch || COMFYUI_DISPATCH_DEFAULT,
                ...(keepComfyWorkflowId ? { comfyui_workflow_id: keepComfyWorkflowId } : {}),
              }
            : {}),
          ...(finalResModelMapping ? { res_model_mapping: finalResModelMapping } : {}),
        };

    const data = {
      ...values,
      models: reliableModels,
      provider_type: values.provider_type || 'custom',
      model_mapping: finalMapping,
      user_groups: isExcludeMode ? [] : reliableLevels,
      exclude_user_groups: isExcludeMode ? reliableLevels : [],
      config: finalConfig,
      sort_order: values.sort_order || 0,
      category_id: values.category_id ?? null,
      priority: values.priority || 0,
      rate: typeof values.rate === 'number' ? values.rate : 1.0,
      quota_limit: (!enableQuota || values.quota_limit === undefined || values.quota_limit === null) ? -1 : Number(values.quota_limit),
      daily_quota_limit: (!enableQuota || values.daily_quota_limit === undefined || values.daily_quota_limit === null) ? -1 : Number(values.daily_quota_limit),
      weekly_quota_limit: (!enableQuota || values.weekly_quota_limit === undefined || values.weekly_quota_limit === null) ? -1 : Number(values.weekly_quota_limit),
      monthly_quota_limit: (!enableQuota || values.monthly_quota_limit === undefined || values.monthly_quota_limit === null) ? -1 : Number(values.monthly_quota_limit),
      preset_id,
    };
    if (enableQuota) {
      const hierarchyErr = validateQuotaHierarchy(data);
      if (hierarchyErr) {
        message.error(hierarchyErr);
        setSubmitting(false);
        return;
      }
    }
    delete data.level_select;
    data.models = reliableModels;

    // 插件上游不落库密钥/基址，选渠时按凭证或服务节点实时覆盖
    if (data.provider_type === 'volcengine' || data.provider_type === 'comfyui') {
      data.api_key = '';
      data.base_url = '';
    }

    try {
      if (editingChannel) {
        // 密钥未修改（与加载时原值相同）或为空时不提交，防止覆盖（插件上游例外：必须清空）
        if (data.provider_type !== 'volcengine' && data.provider_type !== 'comfyui' && (!data.api_key || data.api_key === (editingChannel as any).api_key)) {
          delete data.api_key;
        }
        await request.put(`/channels/${editingChannel.id}`, data);
        message.success(t('common.success'));
      } else {
        await request.post('/channels', data);
        message.success(t('common.success'));
      }
      handleCloseModal();
      fetchChannels();
    } catch (e) {
      console.error(e);
    } finally {
      setSubmitting(false);
    }
  };

  const handleUpdateSortOrder = async (id: number, sort_order: number) => {
    try {
      await request.put(`/channels/${id}`, { sort_order });
      // Update local state without full refresh for instant feedback
      setChannels(prev => prev.map(c => c.id === id ? { ...c, sort_order } : c));
      message.success('排序已更新');
    } catch (e) {
      console.error(e);
      message.error('排序更新失败');
    }
  };

  /** 解析渠道 config（兼容 string / object） */
  const parseChannelConfig = (record: Channel): Record<string, any> => {
    try {
      if (!record.config) return {};
      return typeof record.config === 'string' ? JSON.parse(record.config) : record.config;
    } catch {
      return {};
    }
  };

  /** 画质增强上游：凭证名称与基址 */
  const resolveVolcEnhanceUpstream = (record: Channel) => {
    const cfg = parseChannelConfig(record);
    const credId = cfg.volcengine_enhance_credential_id;
    const cred = volcengineEnhanceKeys.find((k) => k.id === credId);
    return {
      credId,
      name: cred?.name || (credId ? `凭证 #${credId}` : '未绑定凭证'),
      baseUrl: cred?.base_url || '',
    };
  };

  const resolveComfyuiUpstream = (record: Channel) => {
    const cfg = parseChannelConfig(record);
    const ids = parseComfyuiServerIds(cfg, comfyuiWorkflows);
    if (ids.length > 1) {
      const names = ids.map((id) => comfyuiServers.find((s) => s.id === id)?.name || `#${id}`);
      const dispatch = comfyuiDispatchRules.find((r) => r.code === cfg.comfyui_dispatch)?.name
        || cfg.comfyui_dispatch
        || '权重优先';
      return { name: `${ids.length} 个节点 · ${dispatch}`, baseUrl: names.join('、') };
    }
    const sid = ids[0];
    const server = comfyuiServers.find((s) => s.id === sid);
    if (server) {
      return { name: server.name, baseUrl: server.base_url || '' };
    }
    return {
      name: sid ? `节点 #${sid}` : '未绑定服务节点',
      baseUrl: '',
    };
  };

  const columns = [
    {
      title: '渠道分组名称',
      key: 'name_and_aid',
      render: (_: any, record: Channel) => (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 1, minWidth: 120 }}>
          <Text strong style={{ fontSize: 13, lineHeight: 1.25 }}>{record.name}</Text>
          <Text type="secondary" style={{ fontSize: 11, lineHeight: 1.2, fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace', opacity: 0.75 }}>
            AID: {record.group_aid || '-'}
          </Text>
        </div>
      ),
    },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      width: 75,
      render: (status: number) => (
        <Space size={5} style={{ color: status === 1 ? '#52c41a' : '#ff4d4f' }}>
          <div style={{ width: 6, height: 6, borderRadius: '50%', backgroundColor: status === 1 ? '#52c41a' : '#ff4d4f' }} />
          <span style={{ fontSize: 12 }}>{status === 1 ? '启用' : '禁用'}</span>
        </Space>
      ),
    },
    {
      title: '优先级',
      dataIndex: 'priority',
      key: 'priority',
      width: 75,
      sorter: (a: Channel, b: Channel) => (a.priority || 0) - (b.priority || 0),
      render: (priority: number) => <Text type="secondary" style={{ fontSize: 12 }}>{priority || 0}</Text>,
    },
    {
      title: '支持等级',
      key: 'user_groups',
      render: (_: any, record: Channel) => {
        const groups = record.user_groups;
        const excludeGroups = record.exclude_user_groups;
        const resolveName = (idStr: string) => {
          const lv = availableUserLevels.find((l: any) => l.id.toString() === idStr || l.group_key === idStr);
          return lv ? lv.name : idStr;
        };
        const tagStyle: React.CSSProperties = { borderRadius: 4, margin: 0, padding: '0 5px', fontSize: 11, height: 19, lineHeight: '17px' };
        if (excludeGroups && excludeGroups.length > 0) {
          const visible = excludeGroups.slice(0, 2);
          const hiddenCount = excludeGroups.length - visible.length;
          const allNames = excludeGroups.map(resolveName).join(', ');
          return (
            <div style={{ display: 'flex', flexWrap: 'nowrap', gap: 3, alignItems: 'center' }}>
              <Tag color="orange" style={tagStyle}>排除</Tag>
              {visible.map((id: string) => (
                <Tag key={id} color="red" style={{ ...tagStyle, opacity: 0.85 }}>{resolveName(id)}</Tag>
              ))}
              {hiddenCount > 0 && (
                <Tooltip title={`排除等级: ${allNames}`}>
                  <Tag style={{ ...tagStyle, cursor: 'pointer' }}>+{hiddenCount}</Tag>
                </Tooltip>
              )}
            </div>
          );
        }
        if (!groups || groups.length === 0) {
          return <Tag color="green" style={tagStyle}>全部等级</Tag>;
        }
        const visible = groups.slice(0, 2);
        const hiddenCount = groups.length - visible.length;
        const allNames = groups.map(resolveName).join(', ');
        return (
          <div style={{ display: 'flex', flexWrap: 'nowrap', gap: 3, alignItems: 'center' }}>
            {visible.map((id: string) => (
              <Tag key={id} color="blue" style={tagStyle}>{resolveName(id)}</Tag>
            ))}
            {hiddenCount > 0 && (
              <Tooltip title={`允许等级: ${allNames}`}>
                <Tag style={{ ...tagStyle, cursor: 'pointer' }}>+{hiddenCount}</Tag>
              </Tooltip>
            )}
          </div>
        );
      },
    },
    {
      title: '消耗 / 额度',
      key: 'quota',
      width: 140,
      render: (_: any, record: Channel) => {
        const used = record.quota_used || 0;
        const limit = record.quota_limit ?? -1;
        const dailyLimit = record.daily_quota_limit ?? -1;
        const weeklyLimit = record.weekly_quota_limit ?? -1;
        const monthlyLimit = record.monthly_quota_limit ?? -1;
        const { dailyUsed, weeklyUsed, monthlyUsed } = getEffectiveChannelPeriodUsed(record, quotaTz);
        return renderQuotaRings({
          used,
          limit,
          dailyUsed,
          dailyLimit,
          weeklyUsed,
          weeklyLimit,
          monthlyUsed,
          monthlyLimit,
          compact: true,
        });
      }
    },
    {
      title: '使用上游',
      key: 'upstream',
      render: (_: any, record: Channel) => {
        const tagStyle: React.CSSProperties = { borderRadius: 4, margin: 0, padding: '0 4px', fontSize: 10, height: 18, lineHeight: '16px', flexShrink: 0 };
        if (record.provider_type === 'high_availability_group') {
          const parsed = parseChannelConfig(record);
          const subCount = parsed.sub_channels ? parsed.sub_channels.length : 0;
          return (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                <Tag color={activePlugins['high_availability_channel'] ? 'purple' : 'red'} style={tagStyle}>
                  {activePlugins['high_availability_channel'] ? '高可用' : '高可用未开启'}
                </Tag>
                <Text strong style={{ fontSize: 12, lineHeight: 1.25 }}>高可用虚拟渠道组</Text>
              </div>
              <Text type="secondary" style={{ fontSize: 11, lineHeight: 1.2 }}>已绑定 {subCount} 个渠道</Text>
            </div>
          );
        }
        if (record.provider_type === 'volcengine') {
          const { name, baseUrl } = resolveVolcEnhanceUpstream(record);
          return (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 1, maxWidth: 260 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                <Tag color="magenta" style={tagStyle}>画质增强</Tag>
                <Text strong style={{ fontSize: 12, lineHeight: 1.25 }} ellipsis={{ tooltip: name }}>{name}</Text>
              </div>
              {baseUrl ? (
                <Text type="secondary" style={{ fontSize: 11, lineHeight: 1.2 }} ellipsis={{ tooltip: baseUrl }}>
                  {baseUrl}
                </Text>
              ) : null}
            </div>
          );
        }
        if (record.provider_type === 'comfyui') {
          const { name, baseUrl } = resolveComfyuiUpstream(record);
          return (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 1, maxWidth: 260 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                <Tag color="geekblue" style={tagStyle}>ComfyUI</Tag>
                <Text strong style={{ fontSize: 12, lineHeight: 1.25 }} ellipsis={{ tooltip: name }}>{name}</Text>
              </div>
              {baseUrl ? (
                <Text type="secondary" style={{ fontSize: 11, lineHeight: 1.2 }} ellipsis={{ tooltip: baseUrl }}>
                  {baseUrl}
                </Text>
              ) : null}
            </div>
          );
        }
        if (record.preset_id) {
          const preset = presets.find(p => p.id === record.preset_id);
          const presetDisabled = preset && (preset.status ?? 1) !== 1;
          return (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                <Tag color="cyan" style={tagStyle}>预设渠道</Tag>
                {presetDisabled && <Tag color="error" style={tagStyle}>已禁用</Tag>}
                <Text strong style={{ fontSize: 12, lineHeight: 1.25 }}>{preset ? preset.name : '未知预设'}</Text>
                {renderAssetBindingTag(record.preset_id, tagStyle)}
              </div>
              <Text type="secondary" style={{ fontSize: 11, lineHeight: 1.2, fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace' }}>
                {preset?.yid ? `YID: ${preset.yid}` : `ID: ${record.preset_id}`}
              </Text>
            </div>
          );
        }
        return <Text type="secondary" style={{ fontSize: 12 }}>-</Text>;
      }
    },
    {
      title: '渠道分类',
      dataIndex: 'category_id',
      key: 'category_id',
      render: (categoryId: number | null) => {
        const name = resolveCategoryName(categoryId);
        return name ? (
          <Tag style={{ margin: 0, padding: '0 5px', fontSize: 11, height: 19, lineHeight: '17px', borderRadius: 4 }}>
            {name}
          </Tag>
        ) : <Text type="secondary" style={{ fontSize: 12 }}>-</Text>;
      },
    },
    {
      title: '最后修改',
      dataIndex: 'updated_at',
      key: 'updated_at',
      sorter: (a: Channel, b: Channel) => {
        const ta = a.updated_at || a.created_at || '';
        const tb = b.updated_at || b.created_at || '';
        return ta.localeCompare(tb);
      },
      render: (t: string, record: Channel) => {
        const time = t || record.created_at;
        if (!time) return <Text type="secondary">-</Text>;
        const d = new Date(time);
        return <Text type="secondary" style={{ fontSize: 12, whiteSpace: 'nowrap' }}>{d.toLocaleString('zh-CN', { year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' })}</Text>;
      },
    },
    {
      title: '页面排序',
      dataIndex: 'sort_order',
      key: 'sort_order',
      width: 80,
      sorter: (a: Channel, b: Channel) => (a.sort_order || 0) - (b.sort_order || 0),
      render: (sort_order: number) => <Text type="secondary" style={{ fontSize: 12 }}>{sort_order || 0}</Text>,
    },
    {
      title: '操作',
      key: 'actions',
      align: 'center' as const,
      width: 140,
      render: (_: unknown, record: Channel) => (
        <Space size={2} style={{ justifyContent: 'center', width: '100%' }}>
          <Tooltip title={record.status === 1 ? '点击禁用' : '点击启用'}>
            <Button
              type="text"
              size="small"
              className="channel-table-action-btn"
              icon={record.status === 1
                ? <PlayCircleOutlined style={{ color: '#52c41a' }} />
                : <StopOutlined style={{ color: '#ff4d4f' }} />}
              onClick={() => handleToggleStatus(record)}
            />
          </Tooltip>
          {record.provider_type === 'high_availability_group' && (
            <Tooltip title="重置熔断">
              <Button
                type="text"
                size="small"
                className="channel-table-action-btn"
                icon={<ReloadOutlined style={{ color: '#1890ff' }} />}
                onClick={() => handleResetMeltdown(record.id)}
                loading={meltdownLoading[record.id]}
              />
            </Tooltip>
          )}
          <Tooltip title="测试">
            <Button
              type="text"
              size="small"
              className="channel-table-action-btn"
              icon={<ExperimentOutlined />}
              onClick={() => handleTest(record)}
            />
          </Tooltip>
          <Tooltip title="清零额度">
            <Popconfirm title="确定清零该渠道的总/日/周/月已用额度吗？" onConfirm={() => handleResetQuota(record.id)}>
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

  /** 额度圆环：与上游渠道配置预设一致（总/月/周/日固定位、蓝阶、100% 红） */
  const quotaRingPercent = (used: number, limit: number) => {
    if (limit < 0) return 0;
    if (limit === 0) return used > 0 ? 100 : 0;
    return Math.min(100, Math.round((used / limit) * 100));
  };

  const quotaRingBlue: Record<string, string> = {
    total: '#1d4ed8',
    month: '#2563eb',
    week: '#3b82f6',
    day: '#60a5fa',
  };

  const renderQuotaRings = (opts: {
    used: number;
    limit: number;
    dailyUsed: number;
    dailyLimit: number;
    weeklyUsed: number;
    weeklyLimit: number;
    monthlyUsed: number;
    monthlyLimit: number;
    compact?: boolean;
  }) => {
    const compact = opts.compact ?? false;
    const fmt = (n: number) => (Number.isInteger(n) ? String(n) : n.toFixed(6));
    const items = [
      { key: 'total', label: '总', used: opts.used, limit: opts.limit },
      { key: 'month', label: '月', used: opts.monthlyUsed, limit: opts.monthlyLimit },
      { key: 'week', label: '周', used: opts.weeklyUsed, limit: opts.weeklyLimit },
      { key: 'day', label: '日', used: opts.dailyUsed, limit: opts.dailyLimit },
    ];
    const hasAnyConfigured = items.some((item) => item.limit >= 0);

    const slotWidth = compact ? 28 : 40;
    const ringSize = compact ? 24 : 36;
    const ringStroke = compact ? 5 : 10;
    const slotStyle: React.CSSProperties = {
      width: slotWidth,
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      gap: compact ? 1 : 2,
      cursor: 'default',
    };
    const labelStyle: React.CSSProperties = {
      fontSize: compact ? 9 : 10,
      color: isLight ? 'rgba(0,0,0,0.4)' : 'rgba(255,255,255,0.45)',
      lineHeight: 1,
      transform: compact ? 'scale(0.92)' : undefined,
    };

    return (
      <div
        className="channel-quota-rings"
        style={{
          display: 'grid',
          gridTemplateColumns: `repeat(4, ${slotWidth}px)`,
          gap: compact ? 3 : 4,
          alignItems: 'center',
          justifyContent: 'start',
          width: compact ? 122 : 172,
        }}
      >
        {items.map((item, index) => {
          const configured = item.limit >= 0;
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
            ? `额度：${currencySymbol}${fmt(item.used)} / ∞（无限）`
            : `${item.label}额度：${currencySymbol}${fmt(item.used)} / ${currencySymbol}${fmt(Number(item.limit))}（${pct}%）`;
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
                  trailColor={isLight ? '#e4e4e7' : 'rgba(255,255,255,0.12)'}
                  format={() => (
                    <span
                      style={{
                        fontSize: compact ? (showUnlimited ? 9 : 8) : (showUnlimited ? 11 : 10),
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

  const hasEditParam = new URLSearchParams(window.location.search).has('edit');

  return (
    <Card variant="borderless">
      <style>{`
        .channel-card-disabled {
          opacity: 0.62;
          filter: grayscale(18%);
        }
        .channels-grid-list .ant-list-items {
          display: grid !important;
          grid-template-columns: repeat(auto-fill, minmax(248px, 1fr)) !important;
          gap: 10px !important;
        }
        .channels-grid-list .ant-list-item {
          margin: 0 !important;
          padding: 0 !important;
          width: 100% !important;
          border-block-end: none !important;
        }
        .channel-dash-card {
          transition: border-color 0.15s ease, box-shadow 0.15s ease, transform 0.15s ease;
          cursor: default;
        }
        .channel-dash-card:hover {
          border-color: ${isLight ? 'rgba(22,119,255,0.35)' : 'rgba(105,177,255,0.35)'} !important;
          box-shadow: ${isLight ? '0 4px 14px rgba(15,23,42,0.08)' : '0 4px 16px rgba(0,0,0,0.35)'} !important;
          transform: translateY(-1px);
        }
        .channel-dash-card .channel-dash-actions {
          opacity: 0.72;
          transition: opacity 0.15s ease;
        }
        .channel-dash-card:hover .channel-dash-actions {
          opacity: 1;
        }
        .channel-dash-card .channel-dash-action-btn {
          width: 24px !important;
          height: 24px !important;
          min-width: 24px !important;
          padding: 0 !important;
          display: inline-flex !important;
          align-items: center;
          justify-content: center;
          border-radius: 6px !important;
        }
        .channel-dash-card .channel-dash-action-btn:hover {
          background: ${isLight ? 'rgba(0,0,0,0.06)' : 'rgba(255,255,255,0.08)'} !important;
        }
        @keyframes meltdownPulse {
          0%, 100% { opacity: 1; box-shadow: 0 0 0 0 rgba(255, 77, 79, 0.5); }
          50% { opacity: 0.6; box-shadow: 0 0 0 4px rgba(255, 77, 79, 0); }
        }
        .meltdown-pulse-dot {
          animation: meltdownPulse 1.5s ease-in-out infinite;
        }
        .channel-groups-table .ant-table,
        .channel-groups-table .ant-table-container,
        .channel-groups-table .ant-table-content,
        .channel-groups-table table {
          border-collapse: collapse !important;
          border-spacing: 0 !important;
        }
        .channel-groups-table .ant-table-thead {
          background: ${isLight ? '#f9fafb' : '#18181b'} !important;
        }
        .channel-groups-table .ant-table-thead > tr {
          height: 28px !important;
          background: ${isLight ? '#f9fafb' : '#18181b'} !important;
        }
        .channel-groups-table .ant-table-thead > tr > th,
        .channel-groups-table .ant-table-thead > tr > th.ant-table-cell {
          padding: 3px 8px !important;
          height: 28px !important;
          line-height: 20px !important;
          font-size: 12px !important;
          font-weight: 600 !important;
          background: ${isLight ? '#f9fafb' : '#18181b'} !important;
          border-bottom: 1px solid ${isLight ? 'rgba(0,0,0,0.06)' : 'rgba(255,255,255,0.08)'} !important;
          color: ${isLight ? '#64748b' : '#a1a1aa'} !important;
        }
        .channel-groups-table .ant-table-thead .ant-table-column-sorters {
          padding: 0 !important;
          margin: 0 !important;
          height: 20px !important;
          display: inline-flex !important;
          align-items: center !important;
        }
        .channel-groups-table .ant-table-thead .ant-table-column-title {
          line-height: 20px !important;
          font-size: 12px !important;
          font-weight: 600 !important;
        }
        .channel-groups-table .ant-table-thead .ant-table-column-sorter {
          margin-inline-start: 3px !important;
          font-size: 9px !important;
        }
        .channel-groups-table .ant-table-thead > tr > th::before {
          display: none !important;
        }
        .channel-groups-table .ant-table-tbody {
          margin: 0 !important;
          padding: 0 !important;
        }
        .channel-groups-table .ant-table-tbody > tr {
          margin: 0 !important;
          background: transparent !important;
        }
        .channel-groups-table .ant-table-tbody > tr > td {
          padding: 4px 8px !important;
          font-size: 12px !important;
          border-bottom: 1px solid ${isLight ? 'rgba(0,0,0,0.04)' : 'rgba(255,255,255,0.05)'} !important;
          vertical-align: middle !important;
          line-height: 1.3 !important;
        }
        .channel-groups-table .ant-table-tbody > tr:first-child > td {
          border-top: none !important;
        }
        .channel-groups-table .ant-table-measure-row,
        .channel-groups-table .ant-table-measure-row td,
        .channel-groups-table .ant-table-measure-row th,
        .channel-groups-table tr.ant-table-measure-row,
        .channel-groups-table tr.ant-table-measure-row td,
        .channel-groups-table tr.ant-table-measure-row th,
        .channel-groups-table tr.ant-table-measure-row .ant-table-cell {
          padding: 0 !important;
          height: 0 !important;
          font-size: 0 !important;
          line-height: 0 !important;
          border: none !important;
          visibility: hidden !important;
        }
        .channel-groups-table .ant-table-tbody > tr:hover > td {
          background: ${isLight ? 'rgba(0,0,0,0.02)' : 'rgba(255,255,255,0.03)'} !important;
        }
        .channel-groups-table .ant-table-pagination.ant-pagination {
          margin: 10px 0 0 0 !important;
        }
        .channel-table-action-btn {
          width: 22px !important;
          height: 22px !important;
          min-width: 22px !important;
          padding: 0 !important;
          display: inline-flex !important;
          align-items: center;
          justify-content: center;
          border-radius: 4px !important;
          font-size: 12px !important;
        }
        .channel-table-action-btn:hover {
          background: ${isLight ? 'rgba(0,0,0,0.06)' : 'rgba(255,255,255,0.08)'} !important;
        }
      `}</style>
      {!isModalVisible ? (
        <div style={{ width: '100%' }}>
          <div style={{ display: 'flex', flexDirection: screens.xs ? 'column' : 'row', justifyContent: 'space-between', marginBottom: 24, gap: 12 }}>
            <Title level={4} style={{ margin: 0, fontSize: screens.xs ? 18 : 20, fontWeight: 600 }}>{t('channels.title')}</Title>
            <Space wrap>
              <Segmented
                options={[
                  { label: `全部 (${channels.length})`, value: 'all' },
                  { label: `激活 (${channels.filter(c => c.status === 1).length})`, value: '1' },
                  { label: `已禁用 (${channels.filter(c => c.status === 0).length})`, value: '0' },
                ]}
                value={statusFilter === 'all' ? 'all' : statusFilter.toString()}
                onChange={(val) => setStatusFilter(val === 'all' ? 'all' : parseInt(val as string, 10))}
              />
              <Segmented
                options={[
                  { value: 'card', icon: <AppstoreOutlined /> },
                  { value: 'list', icon: <UnorderedListOutlined /> }
                ]}
                value={viewMode}
                onChange={(val) => {
                  setViewMode(val as 'list' | 'card');
                  localStorage.setItem('channels_view_mode', val as string);
                }}
              />
              <Input.Search
                placeholder="搜索 AID 或 名称"
                allowClear
                onSearch={setSearchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                style={{ width: 200 }}
              />
              <Button icon={<SyncOutlined />} onClick={fetchChannels}>{t('common.refresh')}</Button>
              <Button type="default" icon={<ApartmentOutlined />} onClick={() => navigate(`/${adminPath}/channels/model-display`)}>模型渠道显示</Button>
              <Button type="primary" icon={<PlusOutlined />} onClick={handleAdd}>{t('channels.add_channel')}</Button>
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
              <Text type="secondary" style={{ width: 80, flexShrink: 0, fontSize: 13 }}>渠道分类</Text>
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, flexGrow: 1 }}>
                {([
                  { key: 'all' as const, label: '全部', count: channels.length },
                  ...categories.map(c => ({
                    key: c.id,
                    label: c.name,
                    count: channels.filter(ch => ch.category_id === c.id).length,
                  })),
                  {
                    key: 'unclassified' as const,
                    label: '未分类',
                    count: channels.filter(ch => !ch.category_id).length,
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

            {(() => {
              const typeCounts = {
                default: channels.filter(ch => getChannelTypeKey(ch) === 'default').length,
                volcengine: channels.filter(ch => getChannelTypeKey(ch) === 'volcengine').length,
                ha: channels.filter(ch => getChannelTypeKey(ch) === 'ha').length,
                comfyui: channels.filter(ch => getChannelTypeKey(ch) === 'comfyui').length,
              };
              const typeOptions = (
                [
                  { key: 'all' as const, label: '全部', count: channels.length },
                  { key: 'default' as const, label: '预设', count: typeCounts.default },
                  { key: 'volcengine' as const, label: '画质增强', count: typeCounts.volcengine },
                  { key: 'comfyui' as const, label: 'ComfyUI', count: typeCounts.comfyui },
                  { key: 'ha' as const, label: '高可用', count: typeCounts.ha },
                ] as { key: 'all' | 'default' | 'volcengine' | 'ha' | 'comfyui'; label: string; count: number }[]
              ).filter((item) => item.key === 'all' || item.count > 0);

              if (typeOptions.length <= 1) return null;

              return (
                <div style={{ display: 'flex', alignItems: 'center', padding: 0 }}>
                  <Text type="secondary" style={{ width: 80, flexShrink: 0, fontSize: 13 }}>渠道类型</Text>
                  <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, flexGrow: 1 }}>
                    {typeOptions.map((item) => {
                      const selected = typeFilter === item.key;
                      return (
                        <div
                          key={item.key}
                          onClick={() => setTypeFilter(item.key)}
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
                  </div>
                </div>
              );
            })()}
          </div>

          {screens.xs ? (
            <MobileCardList
              dataSource={filteredChannels}
              loading={loading}
              rowKey="id"
              pagination={{ pageSize: 20, showSizeChanger: true, pageSizeOptions: ['10', '20', '50', '100'], showTotal: (total) => `共 ${total} 条` }}
              renderCard={(record: any) => {
                const used = record.quota_used || 0;
                const limit = record.quota_limit ?? -1;
                const dailyLimit = record.daily_quota_limit ?? -1;
                const weeklyLimit = record.weekly_quota_limit ?? -1;
                const monthlyLimit = record.monthly_quota_limit ?? -1;
                const { dailyUsed, weeklyUsed, monthlyUsed } = getEffectiveChannelPeriodUsed(record, quotaTz);
                const groups = record.user_groups;
                const excludeGroups = record.exclude_user_groups;
                return (
                  <MobileCard
                    title={<Text strong>{record.name}</Text>}
                    extra={
                      <Space size={6} style={{ color: record.status === 1 ? '#52c41a' : '#ff4d4f' }}>
                        <div style={{ width: 6, height: 6, borderRadius: '50%', backgroundColor: record.status === 1 ? '#52c41a' : '#ff4d4f' }} />
                        <span style={{ fontSize: 12 }}>{record.status === 1 ? t('common.active') : t('common.disabled')}</span>
                      </Space>
                    }
                  >
                    {record.group_aid && <CardRow label="AID"><Text type="secondary">{record.group_aid}</Text></CardRow>}
                    <CardRow label="支持等级">
                      {excludeGroups && excludeGroups.length > 0 ? (
                        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, alignItems: 'center' }}>
                          <Tag color="orange" style={{ borderRadius: 4, margin: 0, fontSize: 11 }}>排除</Tag>
                          {excludeGroups.map((id: string) => {
                            const lv = availableUserLevels.find((l: any) => l.id.toString() === id || l.group_key === id);
                            return <Tag key={id} color="red" style={{ borderRadius: 4, margin: 0, fontSize: 11, opacity: 0.85 }}>{lv ? lv.name : id}</Tag>;
                          })}
                        </div>
                      ) : (!groups || groups.length === 0) ? (
                        <Tag color="green" style={{ borderRadius: 4, margin: 0, fontSize: 11 }}>全部等级</Tag>
                      ) : (
                        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
                          {groups.map((id: string) => {
                            const lv = availableUserLevels.find((l: any) => l.id.toString() === id || l.group_key === id);
                            return <Tag key={id} color="blue" style={{ borderRadius: 4, margin: 0, fontSize: 11 }}>{lv ? lv.name : id}</Tag>;
                          })}
                        </div>
                      )}
                    </CardRow>
                    <CardRow label="已用/额度">
                      {renderQuotaRings({
                        used,
                        limit,
                        dailyUsed,
                        dailyLimit,
                        weeklyUsed,
                        weeklyLimit,
                        monthlyUsed,
                        monthlyLimit,
                      })}
                    </CardRow>
                    <CardRow label="使用上游">
                      {record.provider_type === 'high_availability_group' ? (
                        (() => {
                          const parsed = parseChannelConfig(record);
                          const subCount = parsed.sub_channels ? parsed.sub_channels.length : 0;
                          return (
                            <Space size={4}>
                              <Tag color={activePlugins['high_availability_channel'] ? 'purple' : 'red'} style={{ borderRadius: 4, margin: 0, fontSize: 10 }}>
                                {activePlugins['high_availability_channel'] ? '高可用' : '高可用插件未开启'}
                              </Tag>
                              <Text style={{ fontSize: 12 }}>
                                高可用虚拟渠道组 (已绑定 {subCount} 个渠道)
                              </Text>
                            </Space>
                          );
                        })()
                      ) : record.provider_type === 'volcengine' ? (
                        (() => {
                          const { name, baseUrl } = resolveVolcEnhanceUpstream(record);
                          return (
                            <Space size={4} wrap>
                              <Tag color="magenta" style={{ borderRadius: 4, margin: 0, fontSize: 10 }}>画质增强</Tag>
                              <Text style={{ fontSize: 12 }}>
                                {name}{baseUrl ? ` (${baseUrl})` : ''}
                              </Text>
                            </Space>
                          );
                        })()
                      ) : record.provider_type === 'comfyui' ? (
                        (() => {
                          const { name, baseUrl } = resolveComfyuiUpstream(record);
                          return (
                            <Space size={4} wrap>
                              <Tag color="geekblue" style={{ borderRadius: 4, margin: 0, fontSize: 10 }}>ComfyUI</Tag>
                              <Text style={{ fontSize: 12 }}>
                                {name}{baseUrl ? ` (${baseUrl})` : ''}
                              </Text>
                            </Space>
                          );
                        })()
                      ) : record.preset_id ? (
                        (() => {
                          const preset = presets.find(p => p.id === record.preset_id);
                          const presetDisabled = preset && (preset.status ?? 1) !== 1;
                          return (
                            <Space size={4} wrap>
                              <Tag color="cyan" style={{ borderRadius: 4, margin: 0, fontSize: 10 }}>预设</Tag>
                              {presetDisabled && <Tag color="error" style={{ borderRadius: 4, margin: 0, fontSize: 10 }}>上游已禁用</Tag>}
                              <Text style={{ fontSize: 12 }}>
                                {preset?.name || '未知预设'} ({preset?.yid ? `YID: ${preset.yid}` : `ID: ${record.preset_id}`})
                              </Text>
                              {renderAssetBindingTag(record.preset_id, { borderRadius: 4, margin: 0, fontSize: 10 })}
                            </Space>
                          );
                        })()
                      ) : (
                        <Text type="secondary">-</Text>
                      )}
                    </CardRow>
                    <CardRow label="页面排序">
                      <Text type="secondary">{record.sort_order || 0}</Text>
                    </CardRow>
                    <CardRow label="渠道分类">
                      {resolveCategoryName(record.category_id) ? (
                        <Tag style={{ margin: 0 }}>{resolveCategoryName(record.category_id)}</Tag>
                      ) : (
                        <Text type="secondary">-</Text>
                      )}
                    </CardRow>
                    <CardRow label="优先级">
                      <Text type="secondary">{record.priority || 0}</Text>
                    </CardRow>

                    <CardRow label="最后修改">
                      <Text type="secondary" style={{ fontSize: 12 }}>{new Date(record.updated_at || record.created_at).toLocaleString('zh-CN', { year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' })}</Text>
                    </CardRow>

                    {/* 熔断状态可视化（仅 HA 渠道且有熔断时显示） */}
                    {record.provider_type === 'high_availability_group' && (() => {
                      const meltdown = meltdownMap[record.id];
                      if (!meltdown) return null;
                      const meltedSubs = (meltdown.sub_channels || []).filter((s: any) => s.is_melted);
                      const channelMelted = meltdown.channel_meltdown?.is_melted;
                      const totalMelted = meltedSubs.length + (channelMelted ? 1 : 0);
                      if (totalMelted === 0) return null;
                      const totalSubs = (meltdown.sub_channels || []).length;
                      return (
                        <div style={{
                          background: isLight ? 'rgba(255, 77, 79, 0.04)' : 'rgba(255, 77, 79, 0.08)',
                          borderRadius: 6,
                          padding: '8px 10px',
                          margin: '4px 0',
                          border: isLight ? '1px solid rgba(255, 77, 79, 0.12)' : '1px solid rgba(255, 77, 79, 0.2)',
                        }}>
                          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
                            <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                              <div className="meltdown-pulse-dot" style={{ width: 7, height: 7, borderRadius: '50%', backgroundColor: '#ff4d4f', flexShrink: 0 }} />
                              <Text style={{ fontSize: 12, color: '#ff4d4f', fontWeight: 500 }}>
                                <ThunderboltOutlined style={{ marginRight: 3 }} />
                                熔断中 {meltedSubs.length}/{totalSubs} 个子渠道
                              </Text>
                            </div>
                            <Popconfirm
                              title="确定要重置所有熔断状态吗？"
                              description="重置后所有子渠道将立即恢复可用"
                              onConfirm={() => handleResetMeltdown(record.id)}
                              okText="重置"
                              cancelText="取消"
                            >
                              <Button
                                type="link"
                                size="small"
                                danger
                                loading={meltdownLoading[record.id]}
                                icon={<ReloadOutlined />}
                                style={{ padding: 0, fontSize: 12, height: 20 }}
                              >
                                重置熔断
                              </Button>
                            </Popconfirm>
                          </div>
                          {meltedSubs.map((sub: any) => (
                            <div key={sub.config_id} style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', fontSize: 12, padding: '2px 0' }}>
                              <Text style={{ fontSize: 12, color: isLight ? 'rgba(0,0,0,0.65)' : 'rgba(255,255,255,0.65)' }} ellipsis>
                                {sub.name}
                              </Text>
                              <Tag color="red" style={{ borderRadius: 4, margin: 0, padding: '0 4px', fontSize: 10, lineHeight: '16px' }}>
                                剩余 {sub.remaining_seconds}s
                              </Tag>
                            </div>
                          ))}
                        </div>
                      );
                    })()}

                    <CardActions>
                      <Tooltip title={record.status === 1 ? '点击禁用' : '点击启用'}>
                        <Button
                          type="text"
                          size="small"
                          icon={record.status === 1
                            ? <PlayCircleOutlined style={{ color: '#52c41a' }} />
                            : <StopOutlined style={{ color: '#ff4d4f' }} />}
                          onClick={() => handleToggleStatus(record)}
                        />
                      </Tooltip>
                      {record.provider_type === 'high_availability_group' && (
                        <Tooltip title="重置熔断">
                          <Button
                            type="text"
                            size="small"
                            icon={<ReloadOutlined style={{ color: '#1890ff' }} />}
                            onClick={() => handleResetMeltdown(record.id)}
                            loading={meltdownLoading[record.id]}
                          />
                        </Tooltip>
                      )}
                      <Tooltip title="测试">
                        <Button type="text" size="small" icon={<ExperimentOutlined />} onClick={() => handleTest(record)} />
                      </Tooltip>
                      <Tooltip title="清零额度">
                        <Popconfirm title="确定清零该渠道的总/日/周/月已用额度吗？" onConfirm={() => handleResetQuota(record.id)}>
                          <Button type="text" size="small" icon={<ClearOutlined />} />
                        </Popconfirm>
                      </Tooltip>
                      <Tooltip title="编辑">
                        <Button type="text" size="small" icon={<EditOutlined />} onClick={() => handleEdit(record)} />
                      </Tooltip>
                      <Tooltip title="删除">
                        <Popconfirm title={t('common.confirm_delete')} onConfirm={() => handleDelete(record.id)}>
                          <Button type="text" size="small" icon={<DeleteOutlined />} danger />
                        </Popconfirm>
                      </Tooltip>
                    </CardActions>
                  </MobileCard>
                );
              }}
            />
          ) : viewMode === 'list' ? (
            <Table
              className="channel-groups-table compact-table"
              size="small"
              dataSource={filteredChannels}
              columns={columns}
              rowKey="id"
              loading={loading}
              pagination={{ pageSize: 20, showSizeChanger: true, pageSizeOptions: ['10', '20', '50', '100'], showTotal: (total) => `共 ${total} 条` }}
              scroll={{ x: 'max-content' }}
            />
          ) : (
            <List
              className="channels-grid-list"
              dataSource={filteredChannels}
              loading={loading}
              pagination={{
                pageSize: 24,
                showSizeChanger: true,
                pageSizeOptions: ['24', '48', '72', '96'],
                showTotal: (total) => `共 ${total} 个渠道`,
                size: 'small',
              }}
              renderItem={(record: Channel) => {
                const used = record.quota_used || 0;
                const limit = record.quota_limit ?? -1;
                const dailyLimit = record.daily_quota_limit ?? -1;
                const weeklyLimit = record.weekly_quota_limit ?? -1;
                const monthlyLimit = record.monthly_quota_limit ?? -1;
                const { dailyUsed, weeklyUsed, monthlyUsed } = getEffectiveChannelPeriodUsed(record, quotaTz);

                const groups = record.user_groups || [];
                const excludeGroups = record.exclude_user_groups || [];
                const resolveName = (idStr: string) => {
                  const lv = availableUserLevels.find((l: any) => l.id.toString() === idStr || l.group_key === idStr);
                  return lv ? lv.name : idStr;
                };
                const levelIds = excludeGroups.length > 0 ? excludeGroups : groups;
                const levelMode = excludeGroups.length > 0 ? 'exclude' : (!groups.length ? 'all' : 'allow');
                const visibleLevels = levelIds.slice(0, 2);
                const moreLevels = levelIds.length - visibleLevels.length;

                const isHa = record.provider_type === 'high_availability_group';
                const hasMeltdown = isHa && meltdownMap[record.id] && (
                  (meltdownMap[record.id].sub_channels || []).some((s: any) => s.is_melted)
                  || meltdownMap[record.id].channel_meltdown?.is_melted
                );
                const categoryName = resolveCategoryName(record.category_id);

                let upstreamTag: React.ReactNode = (
                  <Tag style={{ margin: 0, padding: '0 5px', fontSize: 10, lineHeight: '18px', borderRadius: 4 }}>无上游</Tag>
                );
                if (isHa) {
                  const parsed = parseChannelConfig(record);
                  const subIds = parsed.sub_channels || [];
                  const boundChannels = subIds.map((id: any) => {
                    const p = presets.find((x: any) => x.id === id);
                    return p ? `- ${p.name} (YID: ${p.yid || '无'})` : `- 未知渠道 (ID: ${id})`;
                  });
                  const tip = boundChannels.length > 0
                    ? `高可用虚拟渠道组，已绑定:\n${boundChannels.join('\n')}`
                    : '高可用虚拟渠道组 (未绑定上游)';
                  upstreamTag = (
                    <Tooltip title={<div style={{ whiteSpace: 'pre-wrap' }}>{tip}</div>}>
                      <Tag color={activePlugins['high_availability_channel'] ? 'purple' : 'red'} style={{ margin: 0, padding: '0 5px', fontSize: 10, lineHeight: '18px', borderRadius: 4 }}>
                        {activePlugins['high_availability_channel'] ? `高可用 · ${subIds.length}` : '高可用未开启'}
                      </Tag>
                    </Tooltip>
                  );
                } else if (record.provider_type === 'volcengine') {
                  const { name, baseUrl } = resolveVolcEnhanceUpstream(record);
                  upstreamTag = (
                    <Tooltip title={<div style={{ whiteSpace: 'pre-wrap' }}>{baseUrl ? `画质增强: ${name}\n${baseUrl}` : `画质增强: ${name}`}</div>}>
                      <Tag color="magenta" style={{ margin: 0, padding: '0 5px', fontSize: 10, lineHeight: '18px', borderRadius: 4 }}>画质增强</Tag>
                    </Tooltip>
                  );
                } else if (record.provider_type === 'comfyui') {
                  const { name, baseUrl } = resolveComfyuiUpstream(record);
                  upstreamTag = (
                    <Tooltip title={<div style={{ whiteSpace: 'pre-wrap' }}>{baseUrl ? `ComfyUI: ${name}\n${baseUrl}` : `ComfyUI: ${name}`}</div>}>
                      <Tag color="geekblue" style={{ margin: 0, padding: '0 5px', fontSize: 10, lineHeight: '18px', borderRadius: 4 }}>ComfyUI</Tag>
                    </Tooltip>
                  );
                } else if (record.preset_id) {
                  const preset = presets.find(p => p.id === record.preset_id);
                  const presetDisabled = preset && (preset.status ?? 1) !== 1;
                  upstreamTag = (
                    <Tooltip title={preset ? `预设: ${preset.name} (YID: ${preset.yid || '无'})${presetDisabled ? ' · 已禁用' : ''}` : '未知预设'}>
                      <Tag color={presetDisabled ? 'error' : 'cyan'} style={{ margin: 0, padding: '0 5px', fontSize: 10, lineHeight: '18px', borderRadius: 4 }}>
                        {presetDisabled ? '上游已禁用' : '预设'}
                      </Tag>
                    </Tooltip>
                  );
                }

                return (
                  <List.Item style={{ height: '100%', marginBottom: 0, width: '100%' }}>
                    <Card
                      className={`channel-dash-card${record.status === 0 ? ' channel-card-disabled' : ''}`}
                      onDoubleClick={() => handleEdit(record)}
                      style={{
                        background: isLight ? '#fff' : '#1a1a1a',
                        borderRadius: 8,
                        border: isLight ? '1px solid rgba(0,0,0,0.08)' : '1px solid rgba(255,255,255,0.1)',
                        boxShadow: 'none',
                        display: 'flex',
                        flexDirection: 'column',
                        width: '100%',
                        height: '100%',
                        ...(hasMeltdown ? { borderLeft: '3px solid #ff4d4f' } : {}),
                      }}
                      styles={{ body: { padding: '10px 12px', display: 'flex', flexDirection: 'column', gap: 6, flex: 1 } }}
                    >
                      {/* 头部：状态 + 名称 */}
                      <div style={{ display: 'flex', alignItems: 'center', gap: 6, minHeight: 24 }}>
                        <Tooltip title={record.status === 1 ? '点击禁用' : '点击启用'}>
                          <button
                            type="button"
                            onClick={(e) => { e.stopPropagation(); handleToggleStatus(record); }}
                            style={{
                              width: 8,
                              height: 8,
                              borderRadius: '50%',
                              border: 'none',
                              padding: 0,
                              flexShrink: 0,
                              cursor: 'pointer',
                              backgroundColor: record.status === 1 ? '#52c41a' : (isLight ? '#d1d5db' : 'rgba(255,255,255,0.28)'),
                            }}
                            aria-label={record.status === 1 ? '禁用' : '启用'}
                          />
                        </Tooltip>
                        <Tooltip title={`${record.name}${record.group_aid ? ` · AID ${record.group_aid}` : ''}（双击编辑）`}>
                          <Text
                            strong
                            onClick={() => handleEdit(record)}
                            style={{
                              flex: 1,
                              minWidth: 0,
                              fontSize: 13,
                              lineHeight: '20px',
                              whiteSpace: 'nowrap',
                              overflow: 'hidden',
                              textOverflow: 'ellipsis',
                              color: isLight ? '#111827' : '#f3f4f6',
                              cursor: 'pointer',
                            }}
                          >
                            {record.name}
                          </Text>
                        </Tooltip>
                      </div>

                      {/* 标签行：上游 + 等级 */}
                      <div style={{ display: 'flex', alignItems: 'center', gap: 4, flexWrap: 'wrap', minHeight: 20 }}>
                        {upstreamTag}
                        {levelMode === 'all' ? (
                          <Tag color="green" style={{ margin: 0, padding: '0 5px', fontSize: 10, lineHeight: '18px', borderRadius: 4 }}>全部等级</Tag>
                        ) : (
                          <>
                            {levelMode === 'exclude' && (
                              <Tag color="orange" style={{ margin: 0, padding: '0 5px', fontSize: 10, lineHeight: '18px', borderRadius: 4 }}>排除</Tag>
                            )}
                            {visibleLevels.map((id: string) => (
                              <Tag
                                key={id}
                                color={levelMode === 'exclude' ? 'red' : 'blue'}
                                style={{ margin: 0, padding: '0 5px', fontSize: 10, lineHeight: '18px', borderRadius: 4, maxWidth: 72, overflow: 'hidden', textOverflow: 'ellipsis' }}
                              >
                                {resolveName(id)}
                              </Tag>
                            ))}
                            {moreLevels > 0 && (
                              <Tooltip title={levelIds.slice(2).map(resolveName).join('、')}>
                                <Tag style={{ margin: 0, padding: '0 5px', fontSize: 10, lineHeight: '18px', borderRadius: 4 }}>+{moreLevels}</Tag>
                              </Tooltip>
                            )}
                          </>
                        )}
                      </div>

                      {/* 元信息一行 */}
                      <div
                        style={{
                          display: 'flex',
                          alignItems: 'center',
                          gap: 6,
                          fontSize: 11,
                          color: isLight ? 'rgba(0,0,0,0.4)' : 'rgba(255,255,255,0.4)',
                          lineHeight: 1.2,
                          minHeight: 16,
                        }}
                      >
                        <span>优先级 {record.priority || 0}</span>
                        <span style={{ opacity: 0.45 }}>·</span>
                        <span>排序 {record.sort_order || 0}</span>
                        {categoryName && (
                          <>
                            <span style={{ opacity: 0.45 }}>·</span>
                            <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', maxWidth: 72 }}>{categoryName}</span>
                          </>
                        )}
                      </div>

                      {/* 熔断条（仅有熔断时） */}
                      {hasMeltdown && (() => {
                        const meltdown = meltdownMap[record.id];
                        const meltedSubs = (meltdown.sub_channels || []).filter((s: any) => s.is_melted);
                        const totalSubs = (meltdown.sub_channels || []).length;
                        return (
                          <div
                            style={{
                              display: 'flex',
                              alignItems: 'center',
                              justifyContent: 'space-between',
                              gap: 6,
                              background: isLight ? 'rgba(255,77,79,0.05)' : 'rgba(255,77,79,0.1)',
                              borderRadius: 6,
                              padding: '4px 8px',
                            }}
                          >
                            <div style={{ display: 'flex', alignItems: 'center', gap: 5, minWidth: 0 }}>
                              <div className="meltdown-pulse-dot" style={{ width: 6, height: 6, borderRadius: '50%', backgroundColor: '#ff4d4f', flexShrink: 0 }} />
                              <Text style={{ fontSize: 11, color: '#ff4d4f', fontWeight: 500 }} ellipsis>
                                熔断 {meltedSubs.length}/{totalSubs}
                              </Text>
                            </div>
                            <Popconfirm
                              title="确定重置所有熔断？"
                              onConfirm={() => handleResetMeltdown(record.id)}
                              okText="重置"
                              cancelText="取消"
                            >
                              <Button type="link" size="small" danger loading={meltdownLoading[record.id]} style={{ padding: 0, height: 18, fontSize: 11 }}>
                                重置
                              </Button>
                            </Popconfirm>
                          </div>
                        );
                      })()}

                      {/* 额度环图（仅已配置） */}
                      <div style={{ marginTop: 'auto' }}>
                        {renderQuotaRings({
                          used,
                          limit,
                          dailyUsed,
                          dailyLimit,
                          weeklyUsed,
                          weeklyLimit,
                          monthlyUsed,
                          monthlyLimit,
                        })}
                      </div>

                      {/* 底栏：AID + 操作 */}
                      <div
                        className="channel-dash-actions"
                        style={{
                          display: 'flex',
                          alignItems: 'center',
                          justifyContent: 'space-between',
                          gap: 6,
                          marginTop: 2,
                          paddingTop: 6,
                          borderTop: isLight ? '1px solid rgba(0,0,0,0.06)' : '1px solid rgba(255,255,255,0.08)',
                        }}
                        onClick={(e) => e.stopPropagation()}
                      >
                        <Text
                          type="secondary"
                          style={{
                            flex: 1,
                            minWidth: 0,
                            fontSize: 11,
                            fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace',
                            opacity: 0.75,
                            overflow: 'hidden',
                            textOverflow: 'ellipsis',
                            whiteSpace: 'nowrap',
                            margin: 0,
                          }}
                        >
                          {record.group_aid ? `AID ${record.group_aid}` : '—'}
                        </Text>
                        <div style={{ display: 'flex', alignItems: 'center', gap: 2, flexShrink: 0 }}>
                          {isHa && (
                            <Tooltip title="重置熔断">
                              <Button
                                className="channel-dash-action-btn"
                                type="text"
                                size="small"
                                icon={<ReloadOutlined style={{ fontSize: 12, color: '#1677ff' }} />}
                                loading={meltdownLoading[record.id]}
                                onClick={() => handleResetMeltdown(record.id)}
                              />
                            </Tooltip>
                          )}
                          <Tooltip title="测试">
                            <Button
                              className="channel-dash-action-btn"
                              type="text"
                              size="small"
                              icon={<ExperimentOutlined style={{ fontSize: 12 }} />}
                              onClick={() => handleTest(record)}
                            />
                          </Tooltip>
                          <Tooltip title="编辑">
                            <Button
                              className="channel-dash-action-btn"
                              type="text"
                              size="small"
                              icon={<EditOutlined style={{ fontSize: 12 }} />}
                              onClick={() => handleEdit(record)}
                            />
                          </Tooltip>
                          <Popconfirm title={t('common.confirm_delete')} onConfirm={() => handleDelete(record.id)}>
                            <Tooltip title="删除">
                              <Button
                                className="channel-dash-action-btn"
                                type="text"
                                size="small"
                                danger
                                icon={<DeleteOutlined style={{ fontSize: 12 }} />}
                              />
                            </Tooltip>
                          </Popconfirm>
                        </div>
                      </div>
                    </Card>
                  </List.Item>
                );
              }}
            />
          )}
        </div>
      ) : (
        <div style={{ animation: 'fadeIn 0.3s' }}>
          <div style={{ display: 'flex', alignItems: 'center', marginBottom: 24, gap: 16 }}>
            <Button icon={<ArrowLeftOutlined />} onClick={() => handleCloseModal()}>返回</Button>
            <Title level={3} style={{ margin: 0 }}>
              {editingChannel ? t('channels.edit_channel') : t('channels.add_channel')}
            </Title>
          </div>
          <div style={{ maxWidth: 1600, width: '100%' }}>
            <Spin spinning={hasEditParam && !editingChannel} size="large">
              <Form form={form} layout="vertical" onFinish={handleSave} preserve={true}>
              <Row gutter={24}>
                {/* 左侧基本配置栏 */}
                <Col xs={24} md={10} xl={10}>
                  <div style={{ padding: 16, background: isLight ? '#f9fafb' : 'rgba(255,255,255,0.04)', borderRadius: 8, height: '100%', position: 'sticky', top: 24 }}>
                    <Form.Item name="name" label={
                      <Space>
                        <Text strong>{t('channels.name')}</Text>
                        {editingChannel?.group_aid && (
                          <Text type="secondary" style={{ fontSize: 12 }}>AID: {editingChannel.group_aid}</Text>
                        )}
                      </Space>
                    } rules={[{ required: true }]}>
                      <Input placeholder="e.g. OpenAI Primary" />
                    </Form.Item>

                    <Row gutter={16}>
                      <Col span={12}>
                        <Form.Item name="sort_order" label={<Text strong>页面排序</Text>} initialValue={0}>
                          <InputNumber min={0} max={9999} style={{ width: '100%' }} placeholder="越大越靠前" />
                        </Form.Item>
                      </Col>
                      <Col span={12}>
                        <Form.Item name="category_id" label={<Text strong>渠道分类</Text>}>
                          <Select
                            allowClear
                            placeholder="选择分类"
                            options={activeCategories.map(c => ({ label: c.name, value: c.id }))}
                            dropdownRender={(menu) => (
                              <>
                                {menu}
                                <Divider style={{ margin: '8px 0' }} />
                                <Button
                                  type="link"
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
                      </Col>
                    </Row>

                    <Form.Item name="status" label={<Text strong>状态</Text>} initialValue={1}>
                      <Select>
                        <Option value={1}>启用</Option>
                        <Option value={0}>禁用</Option>
                      </Select>
                    </Form.Item>

                    <Form.Item name="provider_type" style={{ display: 'none' }}><Input /></Form.Item>
                    <Form.Item name="preset_id" style={{ display: 'none' }}><Input /></Form.Item>
                    <Form.Item name="api_key" style={{ display: 'none' }}><Input /></Form.Item>
                    <Form.Item name="base_url" style={{ display: 'none' }}><Input /></Form.Item>

                    <Form.Item shouldUpdate={(prev, curr) => 
                      prev.preset_id !== curr.preset_id || 
                      prev.provider_type !== curr.provider_type ||
                      prev.api_key !== curr.api_key
                    } noStyle>
                      {() => {
                        const providerType = form.getFieldValue('provider_type');
                        const currentPreset = form.getFieldValue('preset_id');
                        const isActive = activeRightPanel === 'presets';

                        let displayType = '无';
                        let displayName = '未选择';
                        let displayDetail: React.ReactNode = '';
                        let displayRate: React.ReactNode = null;

                        if (providerType === 'high_availability_group') {
                          displayType = '高可用组';
                          displayName = '高可用虚拟渠道组';
                          displayDetail = `已绑定 ${selectedSubChannelAids.length} 个渠道`;
                        } else if (currentPreset) {
                          const preset = presets.find(p => p.id === currentPreset);
                          displayType = '预设渠道';
                          displayName = preset ? preset.name : '未知预设';
                          displayDetail = preset?.yid ? `YID: ${preset.yid}` : `ID: ${currentPreset}`;
                          if (preset) {
                            displayRate = (
                              <Space size={4}>
                                {(preset.status ?? 1) !== 1 && <Tag color="error" style={{ margin: 0, borderRadius: 4, fontSize: 10, padding: '0 4px', lineHeight: '18px', border: 'none' }}>已禁用</Tag>}
                                <Tag color="orange" style={{ margin: 0, borderRadius: 4, fontSize: 10, padding: '0 4px', lineHeight: '18px', border: 'none' }}>倍率: {preset.rate ?? 1.0}x</Tag>
                              </Space>
                            );
                          }
                        } else if (providerType === 'volcengine') {
                          displayType = '画质增强';
                          const credId = configObj.volcengine_enhance_credential_id;
                          const cred = volcengineEnhanceKeys.find(k => k.id === credId);
                          displayName = cred ? cred.name : '画质增强密钥';
                          displayDetail = cred ? `基址: ${cred.base_url || '-'}` : '';
                        } else if (providerType === 'comfyui') {
                          displayType = 'ComfyUI';
                          const comfyIds = parseComfyuiServerIds(configObj, comfyuiWorkflows);
                          const dispatchName = comfyuiDispatchRules.find((r) => r.code === (configObj.comfyui_dispatch || COMFYUI_DISPATCH_DEFAULT))?.name
                            || '权重优先';
                          if (comfyIds.length > 1) {
                            displayName = `已选 ${comfyIds.length} 个服务节点`;
                            displayDetail = dispatchName;
                          } else {
                            const sid = comfyIds[0];
                            const server = comfyuiServers.find(s => s.id === sid);
                            displayName = server ? server.name : (sid ? `节点 #${sid}` : '未选择服务节点');
                            displayDetail = server?.base_url ? `基址: ${server.base_url}` : (sid ? dispatchName : '');
                          }
                        }

                        return (
                          <div 
                            onClick={() => setActiveRightPanel('presets')} 
                            style={{ 
                              padding: '12px 16px', 
                              borderRadius: 8, 
                              border: isActive ? '1px solid var(--text)' : (isLight ? '1px solid #e5e4e7' : '1px solid rgba(255,255,255,0.08)'), 
                              background: isActive ? (isLight ? '#f9fafb' : 'rgba(255,255,255,0.04)') : (isLight ? '#fff' : 'rgba(255,255,255,0.02)'), 
                              cursor: 'pointer', 
                              transition: 'all 0.2s',
                              marginBottom: 12
                            }}
                          >
                            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                              <Text strong={isActive} style={{ color: isActive ? 'var(--text)' : 'inherit' }}>
                                <span style={{ color: '#ff4d4f', marginRight: 4, fontFamily: 'SimSun, sans-serif' }}>*</span>
                                选择上游渠道
                              </Text>
                              <span style={{ fontSize: 12, color: isActive ? 'var(--text)' : 'var(--text-secondary)' }}>
                                {displayType !== '无' ? (
                                  <Tag
                                    color={
                                      providerType === 'high_availability_group'
                                        ? 'purple'
                                        : providerType === 'volcengine'
                                          ? 'magenta'
                                          : providerType === 'comfyui'
                                            ? 'geekblue'
                                            : 'cyan'
                                    }
                                    style={{ margin: 0, borderRadius: 4 }}
                                  >
                                    {displayType}
                                  </Tag>
                                ) : '未选择'}
                                <ArrowRightOutlined style={{ marginLeft: 4 }} />
                              </span>
                            </div>
                            {displayType !== '无' && (
                              <>
                                <div style={{ marginTop: 8, padding: '6px 8px', background: isLight ? 'rgba(0,0,0,0.04)' : 'rgba(255,255,255,0.05)', borderRadius: 4, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                                  <span style={{ fontSize: 12, fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1, marginRight: 8 }}>
                                    {displayName}
                                  </span>
                                  <Space size={6} style={{ flexShrink: 0, alignItems: 'center' }}>
                                    {displayRate}
                                    <span style={{ fontSize: 11, color: 'var(--text-secondary)' }}>{displayDetail}</span>
                                    <Button 
                                      type="text" 
                                      size="small" 
                                      icon={<CloseOutlined style={{ fontSize: 10 }} />} 
                                      style={{ width: 20, height: 20, minWidth: 20, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-secondary)', margin: 0, padding: 0 }}
                                      onClick={(e) => {
                                        e.stopPropagation();
                                        setSelectedSubChannelAids([]);
                                        form.setFieldsValue({
                                          preset_id: null,
                                          provider_type: 'custom'
                                        });
                                        setConfigObj(prev => {
                                          const { volcengine_enhance_credential_id, comfyui_workflow_id, comfyui_server_id, comfyui_server_ids, comfyui_dispatch, ...rest } = prev;
                                          return rest;
                                        });
                                        setUpstreamTab('preset');
                                      }}
                                    />
                                  </Space>
                                </div>
                                {providerType === 'high_availability_group' && selectedSubChannelAids.length > 0 && (
                                  <div style={{ marginTop: 6, display: 'flex', flexDirection: 'column', gap: 4 }} onClick={(e) => e.stopPropagation()}>
                                    {presets
                                      .filter(p => selectedSubChannelAids.includes(p.id))
                                      .map((p) => (
                                        <div 
                                          key={p.id} 
                                          style={{ 
                                            display: 'flex', 
                                            justifyContent: 'space-between', 
                                            alignItems: 'center', 
                                            padding: '4px 8px', 
                                            background: isLight ? 'rgba(0,0,0,0.015)' : 'rgba(255,255,255,0.015)', 
                                            borderRadius: 4,
                                            border: isLight ? '1px dashed rgba(0,0,0,0.06)' : '1px dashed rgba(255,255,255,0.06)',
                                            opacity: (p.status ?? 1) !== 1 ? 0.75 : 1,
                                          }}
                                        >
                                          <div style={{ display: 'flex', alignItems: 'center', overflow: 'hidden', maxWidth: '45%' }}>
                                            <span style={{ fontSize: 11, fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={p.name}>
                                              {p.name}
                                            </span>
                                            {p.yid && (
                                              <Typography.Text keyboard style={{ color: '#1677ff', fontSize: 10, margin: 0, padding: '0 4px', lineHeight: '14px', marginLeft: 6, flexShrink: 0 }}>YID: {p.yid}</Typography.Text>
                                            )}
                                            {(p.status ?? 1) !== 1 && (
                                              <Tag color="error" style={{ margin: '0 0 0 6px', fontSize: 10, lineHeight: '14px', height: 16, padding: '0 4px', borderRadius: 2, flexShrink: 0 }}>已禁用</Tag>
                                            )}
                                          </div>
                                          <Space size={4} style={{ flexShrink: 0, alignItems: 'center' }}>
                                            <Tag color="orange" style={{ margin: 0, fontSize: 10, lineHeight: '14px', height: 16, padding: '0 4px', borderRadius: 2 }}>倍率: {p.rate ?? 1.0}x</Tag>
                                            <Tag color="blue" style={{ margin: 0, fontSize: 10, lineHeight: '14px', height: 16, padding: '0 4px', borderRadius: 2 }}>优先级: {effSubPriority(p)}（{isSubPolicyFieldSet(p.id, 'priority') ? '组内' : '全局'}）</Tag>
                                            <Tag color="cyan" style={{ margin: 0, fontSize: 10, lineHeight: '14px', height: 16, padding: '0 4px', borderRadius: 2 }}>权重: {effSubWeight(p)}（{isSubPolicyFieldSet(p.id, 'weight') ? '组内' : '全局'}）</Tag>
                                            <Tooltip title="移除该上游">
                                              <Button
                                                type="text"
                                                size="small"
                                                danger
                                                icon={<CloseOutlined style={{ fontSize: 10 }} />}
                                                style={{ width: 20, height: 20, minWidth: 20, display: 'flex', alignItems: 'center', justifyContent: 'center', margin: 0, padding: 0 }}
                                                onClick={(e) => {
                                                  e.stopPropagation();
                                                  const next = selectedSubChannelAids.filter((id: number) => id !== p.id);
                                                  setSelectedSubChannelAids(next);
                                                  setConfigObj(prev => ({ ...prev, sub_channels: next }));
                                                }}
                                              />
                                            </Tooltip>
                                          </Space>
                                        </div>
                                      ))
                                    }
                                  </div>
                                )}
                                {providerType === 'comfyui' && parseComfyuiServerIds(configObj, comfyuiWorkflows).length > 0 && (
                                  <div style={{ marginTop: 6, display: 'flex', flexDirection: 'column', gap: 4 }} onClick={(e) => e.stopPropagation()}>
                                    {parseComfyuiServerIds(configObj, comfyuiWorkflows).map((id) => {
                                      const s = comfyuiServers.find((x) => x.id === id);
                                      return (
                                        <div
                                          key={id}
                                          style={{
                                            display: 'flex',
                                            justifyContent: 'space-between',
                                            alignItems: 'center',
                                            padding: '4px 8px',
                                            background: isLight ? 'rgba(0,0,0,0.015)' : 'rgba(255,255,255,0.015)',
                                            borderRadius: 4,
                                            border: isLight ? '1px dashed rgba(0,0,0,0.06)' : '1px dashed rgba(255,255,255,0.06)',
                                            opacity: s && s.is_active !== 1 ? 0.75 : 1,
                                          }}
                                        >
                                          <span style={{ fontSize: 11, fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                                            {s?.name || `节点 #${id}`}
                                          </span>
                                          <Space size={4} style={{ flexShrink: 0, alignItems: 'center' }}>
                                            <Tag color="blue" style={{ margin: 0, fontSize: 10, lineHeight: '14px', height: 16, padding: '0 4px', borderRadius: 2 }}>优先级: {s?.priority ?? 0}</Tag>
                                            <Tag color="cyan" style={{ margin: 0, fontSize: 10, lineHeight: '14px', height: 16, padding: '0 4px', borderRadius: 2 }}>权重: {s?.weight ?? 1}</Tag>
                                            <Tooltip title="移除此节点">
                                              <Button
                                                type="text"
                                                size="small"
                                                danger
                                                icon={<CloseOutlined style={{ fontSize: 10 }} />}
                                                style={{ width: 20, height: 20, minWidth: 20, display: 'flex', alignItems: 'center', justifyContent: 'center', margin: 0, padding: 0 }}
                                                onClick={(e) => {
                                                  e.stopPropagation();
                                                  const next = parseComfyuiServerIds(configObj, comfyuiWorkflows).filter((x) => x !== id);
                                                  form.setFieldsValue({
                                                    preset_id: null,
                                                    provider_type: next.length ? 'comfyui' : 'custom',
                                                  });
                                                  setConfigObj(prev => {
                                                    const { volcengine_enhance_credential_id, comfyui_workflow_id, comfyui_server_id, comfyui_server_ids, ...rest } = prev;
                                                    return {
                                                      ...rest,
                                                      comfyui_server_ids: next,
                                                      comfyui_server_id: next[0],
                                                      comfyui_dispatch: prev.comfyui_dispatch || COMFYUI_DISPATCH_DEFAULT,
                                                    };
                                                  });
                                                }}
                                              />
                                            </Tooltip>
                                          </Space>
                                        </div>
                                      );
                                    })}
                                  </div>
                                )}
                              </>
                            )}
                          </div>
                        );
                      }}
                    </Form.Item>


                    <Form.Item label={<Text strong>路由与范围配置</Text>} style={{ marginBottom: 0 }}>
                      <Space direction="vertical" style={{ width: '100%' }} size={12}>
                        {/* Plugin: happyhorse_router 路由切换+节点选择（插件可移除） */}
                        {happyHorseEnabled && hhModule && (
                          <hhModule.HappyHorseRouteConfig
                            enabled={happyHorseEnabled}
                            isRouting={isHappyHorseRouting}
                            configs={happyHorseConfigs}
                            selectedNode={selectedRoutingNode}
                            onSwitchMode={(val: any) => {
                              const isHH = val === 'happyhorse';
                              setIsHappyHorseRouting(isHH);
                              if (isHH) {
                                const activeConfigs = happyHorseConfigs.filter((c: any) => c.is_active === 1);
                                const defaultNode = activeConfigs[0]?.routing_node || '';
                                setSelectedRoutingNode(defaultNode);
                                if (defaultNode) handleModelsChange([defaultNode]);
                              } else {
                                setSelectedRoutingNode(null);
                                handleModelsChange([]);
                              }
                            }}
                            onSelectNode={(nodeVal: any) => {
                              setSelectedRoutingNode(nodeVal);
                              handleModelsChange([nodeVal]);
                            }}
                            isLight={isLight}
                          />
                        )}

                        {/* Models */}
                        <Form.Item shouldUpdate={(prev, curr) => prev.models !== curr.models} noStyle>
                           {() => {
                            const m = form.getFieldValue('models') || [];
                            const isActive = activeRightPanel === 'models';

                            /* Plugin: happyhorse_router 左侧信息卡（插件可移除） */
                            if (isHappyHorseRouting && hhModule) {
                              const activeConfig = happyHorseConfigs.find((c: any) => c.routing_node === selectedRoutingNode);
                              return (
                                <hhModule.HappyHorseStatusCard
                                  activeConfig={activeConfig || null}
                                  selectedNode={selectedRoutingNode}
                                  isLight={isLight}
                                  onClick={() => setActiveRightPanel('models')}
                                />
                              );
                            }

                            return (
                              <div onClick={() => setActiveRightPanel('models')} style={{ padding: '12px 16px', borderRadius: 8, border: isActive ? '1px solid var(--text)' : (isLight ? '1px solid #e5e4e7' : '1px solid rgba(255,255,255,0.08)'), background: isActive ? (isLight ? '#f9fafb' : 'rgba(255,255,255,0.04)') : (isLight ? '#fff' : 'rgba(255,255,255,0.02)'), cursor: 'pointer', transition: 'all 0.2s' }}>
                                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: m.length > 0 ? 8 : 0 }}>
                                  <Text strong={isActive} style={{ color: isActive ? 'var(--text)' : 'inherit' }}>选择模型</Text>
                                  <span style={{ fontSize: 12, color: isActive ? 'var(--text)' : 'var(--text-secondary)' }}>已选 {m.length} 个 <ArrowRightOutlined style={{ marginLeft: 4 }} /></span>
                                </div>
                                {m.length > 0 && (
                                  <div style={{ display: 'flex', flexDirection: 'column', gap: 6, maxHeight: 300, overflowY: 'auto', paddingRight: 4 }}>
                                    {m.map((mid: string) => {
                                      const match = availableModels.find((model: any) => model.mid === mid);
                                      return (
                                        <div key={mid} style={{ padding: '6px 8px', background: isLight ? 'rgba(0,0,0,0.04)' : 'rgba(255,255,255,0.05)', borderRadius: 4, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                                          <span style={{ fontSize: 12, fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1, marginRight: 8 }}>
                                            {match ? match.name : mid}
                                            {(!match || match.is_active === 0) && <Tag color="error" bordered={false} style={{ borderRadius: 4, margin: 0, padding: '0 4px', fontSize: 10, lineHeight: '18px', marginLeft: 8 }}>已禁用</Tag>}
                                            {match && match.remark && <span style={{ color: 'var(--text-secondary)', fontWeight: 'normal', marginLeft: 4 }}>({match.remark})</span>}
                                          </span>
                                          <Space size={4} style={{ flexShrink: 0 }}>
                                            <span style={{ fontSize: 11, color: 'var(--text-secondary)' }}>MID: {match ? match.mid : mid}</span>
                                            <Button 
                                              type="text" 
                                              size="small" 
                                              icon={<CloseOutlined style={{ fontSize: 10 }} />} 
                                              style={{ width: 20, height: 20, minWidth: 20, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-secondary)', margin: 0, padding: 0 }}
                                              onClick={(e) => {
                                                e.stopPropagation();
                                                const next = m.filter((id: string) => id !== mid);
                                                handleModelsChange(next);
                                              }}
                                            />
                                          </Space>
                                        </div>
                                      );
                                    })}
                                  </div>
                                )}
                              </div>
                            );
                          }}
                        </Form.Item>

                        {/* Model Mapping */}
                        <Form.Item shouldUpdate noStyle>
                          {() => {
                            const mapping = modelMappingState || {};
                            const mappedEntries = Object.entries(mapping).filter(([_, v]) => v && String(v).trim());
                            const isActive = activeRightPanel === 'mapping';
                            const providerType = form.getFieldValue('provider_type');
                            const isHaMode = providerType === 'high_availability_group';
                            // 统计 HA 子渠道独立映射数量
                            const haMappingCount = isHaMode ? Object.values(haModelMappingState).reduce((sum, subMap) =>
                              sum + countFilled(subMap), 0) : 0;
                            const resMappingCount = Object.values(resModelMappingState).reduce((sum, scopes) =>
                              sum + Object.values(scopes || {}).reduce((s, m) => s + countFilled(m), 0), 0);
                            return (
                              <div onClick={() => setActiveRightPanel('mapping')} style={{ padding: '12px 16px', borderRadius: 8, border: isActive ? '1px solid var(--text)' : (isLight ? '1px solid #e5e4e7' : '1px solid rgba(255,255,255,0.08)'), background: isActive ? (isLight ? '#f9fafb' : 'rgba(255,255,255,0.04)') : (isLight ? '#fff' : 'rgba(255,255,255,0.02)'), cursor: 'pointer', transition: 'all 0.2s' }}>
                                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: (showMapping && (mappedEntries.length > 0 || haMappingCount > 0 || resMappingCount > 0)) ? 8 : 0 }}>
                                  <Text strong={isActive} style={{ color: isActive ? 'var(--text)' : 'inherit' }}>模型别名映射</Text>
                                  <span style={{ fontSize: 12, color: isActive ? 'var(--text)' : 'var(--text-secondary)' }}>
                                    {showMapping ? <span style={{ fontWeight: 500 }}>已开启 {(mappedEntries.length + haMappingCount + resMappingCount) > 0 ? `(${mappedEntries.length}${haMappingCount > 0 ? `+${haMappingCount}` : ''}${resMappingCount > 0 ? `+R${resMappingCount}` : ''})` : ''}</span> : <span>未开启</span>} <ArrowRightOutlined style={{ marginLeft: 4 }} />
                                  </span>
                                </div>
                                {showMapping && mappedEntries.length > 0 && (
                                  <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                                    {mappedEntries.slice(0, 8).map(([k, v]) => (
                                      <div key={k} style={{ padding: '6px 8px', background: isLight ? 'rgba(0,0,0,0.04)' : 'rgba(255,255,255,0.05)', borderRadius: 4, display: 'flex', alignItems: 'center' }}>
                                        <span style={{ fontSize: 12, fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1, marginRight: 8 }} title={k}>{k}</span>
                                        <span style={{ fontSize: 11, color: 'var(--text-secondary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1, marginRight: 8, textAlign: 'right' }} title={String(v)}>➔ {String(v)}</span>
                                        <Button 
                                          type="text" 
                                          size="small" 
                                          icon={<CloseOutlined style={{ fontSize: 10 }} />} 
                                          style={{ width: 20, height: 20, minWidth: 20, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-secondary)', margin: 0, padding: 0, flexShrink: 0 }}
                                          onClick={(e) => {
                                            e.stopPropagation();
                                            setActiveMappingInputs(prev => prev.filter(id => id !== k));
                                            setModelMappingState(prev => {
                                              const next = { ...prev };
                                              delete next[k];
                                              form.setFieldsValue({ model_mapping: next });
                                              return next;
                                            });
                                          }}
                                        />
                                      </div>
                                    ))}
                                    {mappedEntries.length > 8 && <div style={{ fontSize: 11, color: 'var(--text-secondary)', textAlign: 'center', marginTop: 2 }}>...还有 {mappedEntries.length - 8} 个</div>}
                                  </div>
                                )}
                                {showMapping && isHaMode && haMappingCount > 0 && (
                                  <div style={{ padding: '6px 8px', background: isLight ? 'rgba(22,119,255,0.04)' : 'rgba(22,119,255,0.08)', borderRadius: 4, marginTop: mappedEntries.length > 0 ? 6 : 0, display: 'flex', alignItems: 'center', gap: 4 }}>
                                    <ApartmentOutlined style={{ fontSize: 11, color: '#1677ff' }} />
                                    <span style={{ fontSize: 11, color: '#1677ff', fontWeight: 500 }}>子渠道独立映射: {haMappingCount} 条</span>
                                  </div>
                                )}
                                {showMapping && resMappingCount > 0 && (
                                  <div style={{ padding: '6px 8px', background: isLight ? 'rgba(250,140,22,0.06)' : 'rgba(250,140,22,0.1)', borderRadius: 4, marginTop: (mappedEntries.length > 0 || haMappingCount > 0) ? 6 : 0, display: 'flex', alignItems: 'center', gap: 4 }}>
                                    <SettingOutlined style={{ fontSize: 11, color: '#fa8c16' }} />
                                    <span style={{ fontSize: 11, color: '#fa8c16', fontWeight: 500 }}>分辨率映射: {resMappingCount} 档</span>
                                  </div>
                                )}
                              </div>
                            );
                          }}
                        </Form.Item>

                        {/* User Levels */}
                        <Form.Item shouldUpdate={(prev, curr) => prev.level_select !== curr.level_select} noStyle>
                          {() => {
                            const levels = form.getFieldValue('level_select') || [];
                            const isActive = activeRightPanel === 'levels';
                            return (
                              <div onClick={() => setActiveRightPanel('levels')} style={{ padding: '12px 16px', borderRadius: 8, border: isActive ? '1px solid var(--text)' : (isLight ? '1px solid #e5e4e7' : '1px solid rgba(255,255,255,0.08)'), background: isActive ? (isLight ? '#f9fafb' : 'rgba(255,255,255,0.04)') : (isLight ? '#fff' : 'rgba(255,255,255,0.02)'), cursor: 'pointer', transition: 'all 0.2s' }}>
                                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: levels.length > 0 ? 8 : 0 }}>
                                  <Text strong={isActive} style={{ color: isActive ? 'var(--text)' : 'inherit' }}>{isExcludeMode ? '不支持用户等级' : '支持用户等级'}</Text>
                                  <span style={{ fontSize: 12, color: isActive ? 'var(--text)' : 'var(--text-secondary)' }}>
                                    <Tag style={{ marginRight: 4, border: 'none', background: isLight ? 'rgba(0,0,0,0.06)' : 'rgba(255,255,255,0.1)', color: 'var(--text)' }}>{isExcludeMode ? '排除模式' : '允许模式'}</Tag>
                                    <ArrowRightOutlined />
                                  </span>
                                </div>
                                {levels.length > 0 ? (
                                  <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                                    {levels.slice(0, 8).map((idStr: string) => {
                                      const lv = availableUserLevels.find((l: any) => l.id.toString() === idStr || l.group_key === idStr);
                                      return (
                                      <div key={idStr} style={{ padding: '6px 8px', background: isLight ? 'rgba(0,0,0,0.04)' : 'rgba(255,255,255,0.05)', borderRadius: 4, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                                        <span style={{ fontSize: 12, fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1, marginRight: 8 }}>{lv ? `${lv.name} (${lv.discount}x)` : '未知等级'}</span>
                                        <Space size={4} style={{ flexShrink: 0 }}>
                                          <span style={{ fontSize: 11, color: 'var(--text-secondary)' }}>ULID: {idStr.padStart(4, '0')}</span>
                                          <Button 
                                            type="text" 
                                            size="small" 
                                            icon={<CloseOutlined style={{ fontSize: 10 }} />} 
                                            style={{ width: 20, height: 20, minWidth: 20, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-secondary)', margin: 0, padding: 0 }}
                                            onClick={(e) => {
                                              e.stopPropagation();
                                              const next = levels.filter((id: string) => id !== idStr);
                                              form.setFieldsValue({ level_select: next });
                                              levelsRef.current = next;
                                            }}
                                          />
                                        </Space>
                                      </div>
                                      );
                                    })}
                                    {levels.length > 8 && <div style={{ fontSize: 11, color: 'var(--text-secondary)', textAlign: 'center', marginTop: 2 }}>...还有 {levels.length - 8} 个</div>}
                                  </div>
                                ) : (
                                  <Text type="secondary" style={{ fontSize: 12 }}>{isExcludeMode ? '未排除任何等级 (全部允许)' : '当前全部用户等级允许'}</Text>
                                )}
                              </div>
                            );
                          }}
                        </Form.Item>
                      </Space>
                    </Form.Item>

                    <div style={{ marginTop: 24 }}>
                      <Text strong style={{ display: 'block', marginBottom: 12 }}>调度策略</Text>
                      <Row gutter={12}>
                        <Col span={12}>
                          <Form.Item name="priority" label="优先级" initialValue={0}>
                            <InputNumber style={{ width: '100%' }} />
                          </Form.Item>
                        </Col>
                        <Col span={12}>
                          <Form.Item name="weight" label="权重" initialValue={1}>
                            <InputNumber style={{ width: '100%' }} />
                          </Form.Item>
                        </Col>
                        <Col span={24}>
                          <Form.Item label="额度配置" style={{ marginBottom: 12 }}>
                            <Switch 
                              checked={enableQuota}
                              onChange={(checked) => {
                                setEnableQuota(checked);
                                if (!checked) {
                                  form.setFieldsValue({
                                    quota_limit: -1,
                                    daily_quota_limit: -1,
                                    weekly_quota_limit: -1,
                                    monthly_quota_limit: -1,
                                  });
                                } else {
                                  form.setFieldsValue({
                                    quota_limit: -1,
                                    daily_quota_limit: -1,
                                    weekly_quota_limit: -1,
                                    monthly_quota_limit: -1,
                                  });
                                }
                              }}
                              checkedChildren="已开启限额"
                              unCheckedChildren="默认不限额"
                            />
                          </Form.Item>
                        </Col>
                        {enableQuota && (
                          <Col span={24}>
                            <Form.Item
                              noStyle
                              shouldUpdate={(prev, curr) =>
                                prev.quota_limit !== curr.quota_limit ||
                                prev.daily_quota_limit !== curr.daily_quota_limit ||
                                prev.weekly_quota_limit !== curr.weekly_quota_limit ||
                                prev.monthly_quota_limit !== curr.monthly_quota_limit
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
                                return (
                                  <Row gutter={12}>
                                    <Col span={12}>
                                      <Form.Item
                                        name="quota_limit"
                                        label="总额度"
                                        initialValue={-1}
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
                                    </Col>
                                    <Col span={12}>
                                      <Form.Item
                                        name="monthly_quota_limit"
                                        label="月额度"
                                        initialValue={-1}
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
                                    </Col>
                                    <Col span={12}>
                                      <Form.Item
                                        name="weekly_quota_limit"
                                        label="周额度"
                                        initialValue={-1}
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
                                    </Col>
                                    <Col span={12}>
                                      <Form.Item
                                        name="daily_quota_limit"
                                        label="日额度"
                                        initialValue={-1}
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
                                    </Col>
                                  </Row>
                                );
                              }}
                            </Form.Item>
                          </Col>
                        )}
                      </Row>
                      <Form.Item name="rate" initialValue={1.0} style={{ display: 'none' }}><InputNumber /></Form.Item>

                      {/* ─── 存储设置 ─── */}
                      <Divider style={{ margin: '12px 0 8px' }}>存储设置</Divider>
                      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
                        <div>
                          <Text strong style={{ fontSize: 13 }}>开启 TOS 资源存储</Text>
                          <div style={{ fontSize: 12, color: 'var(--text-secondary)', marginTop: 2 }}>
                            开启后，图片/视频响应资源自动上传到 TOS 并返回永久 URL
                          </div>
                        </div>
                        <Switch
                          checked={configObj.tos_storage_enabled || false}
                          onChange={(v) => setConfigObj({ ...configObj, tos_storage_enabled: v, tos_storage_days: configObj.tos_storage_days ?? 1 })}
                        />
                      </div>
                      {configObj.tos_storage_enabled && (
                        <div style={{ marginTop: 8 }}>
                          <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
                            <Text style={{ fontSize: 13, whiteSpace: 'nowrap' }}>存储有效期</Text>
                            <InputNumber
                              min={0} max={365}
                              value={configObj.tos_storage_days ?? 1}
                              onChange={(v) => setConfigObj({ ...configObj, tos_storage_days: v })}
                              addonAfter="天"
                              style={{ width: 140 }}
                            />
                            <Text type="secondary" style={{ fontSize: 11 }}>0 = 永久保留</Text>
                          </div>
                          <Alert
                            type="info"
                            showIcon
                            style={{ fontSize: 12, padding: '6px 12px' }}
                            message="使用站点「系统设置 → 存储设置」中配置的 TOS 信息。请确保已正确配置。"
                          />
                        </div>
                      )}
                    </div>
                  </div>
                </Col>

                {/* 右侧动态面板 */}
                <Col xs={24} md={14} xl={14}>
                  <div style={{ padding: 24, background: isLight ? '#fff' : 'rgba(255,255,255,0.02)', border: isLight ? '1px solid #e5e4e7' : '1px solid rgba(255,255,255,0.08)', borderRadius: 8, minHeight: 600 }}>
                    
                    <div style={{ display: activeRightPanel === 'models' ? 'block' : 'none', animation: 'fadeIn 0.2s' }}>
                      {/* Plugin: happyhorse_router 右侧详情面板（插件可移除） */}
                      {isHappyHorseRouting && hhModule ? (() => {
                        const activeConfig = happyHorseConfigs.find((c: any) => c.routing_node === selectedRoutingNode);
                        return (
                          <hhModule.HappyHorseDetailPanel
                            activeConfig={activeConfig || null}
                            selectedNode={selectedRoutingNode}
                            isLight={isLight}
                          />
                        );
                      })() : (
                        <>
                          <Form.Item name="models" rules={[{ required: true, message: '请选择至少一个模型' }]} style={{ marginBottom: 0 }} hidden>
                            <Select mode="multiple" />
                          </Form.Item>
                          <ModelSelector
                            selectedMids={channelModelMids}
                            onSelectionChange={handleModelsChange}
                            onModelsLoaded={(models) => setAvailableModels(models)}
                            allowDuplicateModelId={false}
                            isLightTheme={isLight}
                            title="选择模型"
                          />
                        </>
                      )}
                    </div>

                    <div style={{ display: activeRightPanel === 'presets' ? 'block' : 'none', animation: 'fadeIn 0.2s' }}>
                      <Form.Item shouldUpdate={(prev, curr) => 
                        prev.provider_type !== curr.provider_type || 
                        prev.preset_id !== curr.preset_id ||
                        prev.api_key !== curr.api_key
                      } noStyle>
                        {() => {
                          const providerType = form.getFieldValue('provider_type');
                          return (
                            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16, flexWrap: 'wrap', gap: 8 }}>
                              <Title level={4} style={{ margin: 0 }}>选择上游渠道</Title>
                              <Space size={8} style={{ flexWrap: 'wrap' }}>
                                {/* 按钮一：高可用虚拟渠道组 */}
                                <Tooltip title={!activePlugins['high_availability_channel'] ? '高可用上游渠道系统插件未开启' : ''}>
                                  <Button
                                    type={providerType === 'high_availability_group' ? 'primary' : 'dashed'}
                                    icon={<ApartmentOutlined />}
                                    size="small"
                                    disabled={!activePlugins['high_availability_channel'] && providerType !== 'high_availability_group'}
                                    onClick={() => {
                                      if (providerType === 'high_availability_group') {
                                        // 若已是高可用组，点击则取消选择
                                        setSelectedSubChannelAids([]);
                                        form.setFieldsValue({
                                          preset_id: null,
                                          provider_type: 'custom'
                                        });
                                      } else {
                                        if (!activePlugins['high_availability_channel']) {
                                          message.warning('高可用上游渠道系统插件未开启');
                                          return;
                                        }
                                        // 一键设置为高可用虚拟组
                                        setSelectedSubChannelAids([]);
                                        clearComfyui();
                                        clearVolcengineEnhance();
                                        form.setFieldsValue({
                                          preset_id: -99, // 对应预设 ID
                                          rate: 1.0,
                                          provider_type: 'high_availability_group'
                                        });
                                      }
                                    }}
                                  >
                                    {providerType === 'high_availability_group' ? '已开启高可用上游渠道' : '开启高可用上游渠道'}
                                  </Button>
                                </Tooltip>

                                {/* 按钮四：画质增强渠道 (仅在启用时展示) */}
                                {activePlugins['volcengine_enhance'] && (
                                  <Button
                                    type={upstreamTab === 'volcengine_enhance' ? 'primary' : 'dashed'}
                                    icon={<SettingOutlined />}
                                    size="small"
                                    onClick={() => {
                                      if (upstreamTab === 'volcengine_enhance') {
                                        clearVolcengineEnhance();
                                      } else {
                                        clearComfyui();
                                        setUpstreamTab('volcengine_enhance');
                                        setSelectedSubChannelAids([]);
                                        form.setFieldsValue({
                                          preset_id: null,
                                          provider_type: 'custom'
                                        });
                                      }
                                    }}
                                  >
                                    {upstreamTab === 'volcengine_enhance' ? '已开启火山画质增强渠道' : '开启火山画质增强渠道'}
                                  </Button>
                                )}
                                {activePlugins['comfyui_bridge'] && (
                                  <Button
                                    type={upstreamTab === 'comfyui' ? 'primary' : 'dashed'}
                                    icon={<VideoCameraOutlined />}
                                    size="small"
                                    onClick={() => {
                                      if (upstreamTab === 'comfyui') {
                                        clearComfyui();
                                      } else {
                                        clearVolcengineEnhance();
                                        setUpstreamTab('comfyui');
                                        setSelectedSubChannelAids([]);
                                        form.setFieldsValue({
                                          preset_id: null,
                                          provider_type: 'custom'
                                        });
                                      }
                                    }}
                                  >
                                    {upstreamTab === 'comfyui' ? '已开启 ComfyUI 渠道' : '开启 ComfyUI 渠道'}
                                  </Button>
                                )}
                              </Space>
                            </div>
                          );
                        }}
                      </Form.Item>

                      <Form.Item shouldUpdate={(prev, curr) => prev.preset_id !== curr.preset_id || prev.provider_type !== curr.provider_type} noStyle>
                        {() => {
                          const providerType = form.getFieldValue('provider_type');
                          const currentPreset = form.getFieldValue('preset_id');

                          if (providerType === 'high_availability_group') {
                            const matchPresetSearch = (p: any) =>
                              p.name?.toLowerCase().includes(presetSearchText.toLowerCase()) ||
                              p.provider_type?.toLowerCase().includes(presetSearchText.toLowerCase()) ||
                              String(p.id).includes(presetSearchText) ||
                              (p.yid && String(p.yid).includes(presetSearchText));
                            const searchedPresets = presets.filter(p => p.id !== -99 && matchPresetSearch(p));
                            const statusCounts = {
                              all: searchedPresets.length,
                              active: searchedPresets.filter(p => (p.status ?? 1) === 1).length,
                              disabled: searchedPresets.filter(p => (p.status ?? 1) !== 1).length,
                            };
                            const subCandidates = searchedPresets.filter(p =>
                              upstreamStatusFilter === 'all' || (p.status ?? 1) === upstreamStatusFilter
                            );

                            return (
                              <>
                                {/* 搜索 + 状态筛选 */}
                                <div style={{ marginBottom: 10, display: 'flex', gap: 8, flexWrap: 'wrap', alignItems: 'center' }}>
                                  <Input
                                    placeholder="输入关键字搜索上游渠道配置名称、类型或 YID/ID..."
                                    allowClear
                                    prefix={<SearchOutlined style={{ color: 'var(--text-secondary)' }} />}
                                    value={presetSearchText}
                                    onChange={(e) => setPresetSearchText(e.target.value)}
                                    style={{ flex: 1, minWidth: 200 }}
                                  />
                                  <Segmented
                                    size="small"
                                    options={[
                                      { label: `全部 (${statusCounts.all})`, value: 'all' },
                                      { label: `激活 (${statusCounts.active})`, value: '1' },
                                      { label: `禁用 (${statusCounts.disabled})`, value: '0' },
                                    ]}
                                    value={upstreamStatusFilter === 'all' ? 'all' : String(upstreamStatusFilter)}
                                    onChange={(val) => setUpstreamStatusFilter(val === 'all' ? 'all' : parseInt(val as string, 10) as 0 | 1)}
                                  />
                                </div>

                                <div style={{ marginBottom: 8 }}>
                                  <Alert
                                    message="高可用上游多选绑定"
                                    description={`您可以选择最多 ${haMaxRetries} 个上游配置绑定到该虚拟组（禁用渠道也可选，绑定后运行时不可用）。优先级 / 权重按「组内」生效：可在下方每个已绑上游上单独设置，未设置的跟随该上游的全局值。系统优先使用组内「优先级」最高的一档；同档有多个时按各自「权重」比例随机分流。选满 ${haMaxRetries} 个后，其余配置将被置灰。`}
                                    type="info"
                                    showIcon
                                    style={{ borderRadius: 6, padding: '6px 10px' }}
                                    styles={{
                                      icon: { marginInlineEnd: 8, fontSize: 14, marginTop: 1 },
                                      title: { marginBottom: 2, fontSize: 13, lineHeight: 1.3 },
                                      description: { fontSize: 12, lineHeight: 1.45 },
                                    }}
                                  />
                                </div>

                                <div style={{ marginBottom: 6, display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                                  <Text strong>
                                    绑定物理上游 ({selectedSubChannelAids.length} / {haMaxRetries})
                                  </Text>
                                  <Button
                                    size="small"
                                    disabled={subCandidates.length === 0}
                                    onClick={() => {
                                      const eligibleIds = subCandidates.map(c => c.id);
                                      if (eligibleIds.length === 0) {
                                        message.warning('当前列表没有可绑定的上游配置');
                                        return;
                                      }
                                      const next = eligibleIds.slice(0, haMaxRetries);
                                      setSelectedSubChannelAids(next);
                                      setConfigObj(prev => ({ ...prev, sub_channels: next }));
                                      syncSubPolicyOnBind(next);
                                      if (eligibleIds.length > haMaxRetries) {
                                        message.info(`已按上限选中 ${haMaxRetries} 个上游配置`);
                                      }
                                    }}
                                  >
                                    全选
                                  </Button>
                                  <Button
                                    size="small"
                                    disabled={selectedSubChannelAids.length === 0}
                                    onClick={() => {
                                      setSelectedSubChannelAids([]);
                                      setConfigObj(prev => ({ ...prev, sub_channels: [] }));
                                      syncSubPolicyOnBind([]);
                                    }}
                                  >
                                    取消选择
                                  </Button>
                                  {selectedSubChannelAids.length === 0 && (
                                    <span style={{ color: '#ff4d4f', fontSize: 12, fontWeight: 500 }}>
                                      请至少绑定一个上游配置
                                    </span>
                                  )}
                                </div>

                                {/* 生效顺序预览：按选渠的两级语义（最高档 -> 同档加权随机）计算，只读 */}
                                {selectedSubChannelAids.length > 0 && (() => {
                                  const boundPresets = selectedSubChannelAids
                                    .map((aid: any) => presets.find(p => p.id === aid))
                                    .filter(Boolean) as any[];
                                  if (boundPresets.length === 0) return null;
                                  const meltedSeconds: Record<string, number> = {};
                                  const haMeltdown = editingChannel ? meltdownMap[editingChannel.id] : null;
                                  ((haMeltdown as any)?.sub_channels || []).forEach((s: any) => {
                                    if (s?.is_melted) meltedSeconds[String(s.config_id)] = s.remaining_seconds ?? 0;
                                  });
                                  const tiersMap = new Map<number, any[]>();
                                  boundPresets.forEach(p => {
                                    const pr = Number(effSubPriority(p));
                                    tiersMap.set(pr, [...(tiersMap.get(pr) || []), p]);
                                  });
                                  const tiers = [...tiersMap.entries()].sort((a, b) => b[0] - a[0]);
                                  const topSum = tiers[0][1].reduce((s, p) => s + Math.max(0, Number(effSubWeight(p))), 0);
                                  const topZero = tiers[0][1].some(p => Number(effSubWeight(p)) === 0);
                                  return (
                                    <div style={{ marginBottom: 8, padding: '6px 10px', borderRadius: 6, background: isLight ? 'rgba(0,0,0,0.015)' : 'rgba(255,255,255,0.02)', border: isLight ? '1px solid #e5e4e7' : '1px solid rgba(255,255,255,0.08)' }}>
                                      <div style={{ fontSize: 12, fontWeight: 600, marginBottom: 4 }}>
                                        生效顺序
                                        <span style={{ fontWeight: 400, fontSize: 11, color: 'var(--text-secondary)', marginLeft: 8 }}>
                                          改过组内 P/W 后本组独立运作，不再跟随上游全局值
                                        </span>
                                      </div>
                                      {tiers.map(([pr, list], tierIdx) => {
                                        const sum = list.reduce((s, p) => s + Math.max(0, Number(effSubWeight(p))), 0);
                                        return (
                                          <div key={pr} style={{ marginBottom: 2, display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: 4 }}>
                                            <span style={{ fontSize: 11, color: 'var(--text-secondary)', minWidth: 118 }}>
                                              {tierIdx === 0 ? '主用' : `备用 ${tierIdx}`}（优先级 {pr}）
                                            </span>
                                            {(list as any[]).map((p: any) => {
                                              const w = Math.max(0, Number(effSubWeight(p)));
                                              const pct = sum > 0 ? Math.round((w / sum) * 100) : 0;
                                              const meltedSec = meltedSeconds[String(p.id)];
                                              const isDisabled = (p.status ?? 1) !== 1;
                                              const color = meltedSec !== undefined ? 'error' : (isDisabled ? 'warning' : (tierIdx === 0 ? 'processing' : 'default'));
                                              return (
                                                <Tag key={p.id} color={color} style={{ margin: 0, fontSize: 11, borderRadius: 4 }}>
                                                  {p.name}
                                                  {tierIdx === 0 ? ` ${pct}%` : ''}
                                                  {meltedSec !== undefined ? ' 熔断中' : (isDisabled ? ' 已禁用' : '')}
                                                </Tag>
                                              );
                                            })}
                                          </div>
                                        );
                                      })}
                                      {topSum <= 0 && (
                                        <div style={{ marginTop: 4, fontSize: 11, color: '#ff4d4f' }}>
                                          主用档权重合计为 0：将全量走 {tiers[0][1][0].name}（绑定序第一）
                                        </div>
                                      )}
                                      {topSum > 0 && topZero && (
                                        <div style={{ marginTop: 4, fontSize: 11, color: '#fa8c16' }}>
                                          有权重为 0 的上游不参与分流；0 不等于停用，停用请解绑或禁用该上游预设。
                                        </div>
                                      )}
                                    </div>
                                  );
                                })()}

                                {subCandidates.length === 0 ? (
                                  <div style={{ padding: '40px 0', textAlign: 'center', color: 'var(--text-secondary)', border: '1px dashed #e5e4e7', borderRadius: 8 }}>
                                    暂无可绑定的上游渠道配置（请检查并先在左侧选择或者全局配置中创建）
                                  </div>
                                ) : (
                                  <div style={{ 
                                    maxHeight: 'calc(100vh - 320px)', minHeight: '400px', 
                                    overflowY: 'auto', 
                                    background: isLight ? 'rgba(0,0,0,0.01)' : 'rgba(255,255,255,0.01)', 
                                    padding: '6px', 
                                    borderRadius: 8, 
                                    border: isLight ? '1px solid #e5e4e7' : '1px solid rgba(255,255,255,0.08)' 
                                  }}>
                                    <Space direction="vertical" style={{ width: '100%' }} size={4}>
                                      {subCandidates.map(c => {
                                        const aid = c.id;
                                        const isChecked = selectedSubChannelAids.includes(aid);
                                        const isFull = selectedSubChannelAids.length >= haMaxRetries;
                                        const isConfigDisabled = (c.status ?? 1) !== 1;
                                        const isSlotFull = isFull && !isChecked;

                                        return (
                                          <div 
                                            key={aid} 
                                            style={{ 
                                              display: 'flex', 
                                              alignItems: 'center', 
                                              justifyContent: 'space-between',
                                              padding: '6px 10px', 
                                              borderRadius: 6,
                                              background: isChecked ? (isLight ? 'rgba(22,119,255,0.08)' : 'rgba(22,119,255,0.15)') : 'transparent',
                                              border: isChecked ? '1px solid #91caff' : (isLight ? '1px solid #e5e4e7' : '1px solid rgba(255,255,255,0.08)'),
                                              opacity: isSlotFull ? 0.5 : (isConfigDisabled ? 0.75 : 1),
                                              transition: 'all 0.15s'
                                            }}
                                          >
                                            <Checkbox
                                              checked={isChecked}
                                              disabled={isSlotFull}
                                              onChange={(e) => {
                                                let next = [...selectedSubChannelAids];
                                                if (e.target.checked) {
                                                  if (!next.includes(aid)) next.push(aid);
                                                } else {
                                                  next = next.filter(a => a !== aid);
                                                }
                                                setSelectedSubChannelAids(next);
                                                setConfigObj(prev => ({ ...prev, sub_channels: next }));
                                                syncSubPolicyOnBind(next);
                                              }}
                                              style={{ flex: 1, marginRight: 8 }}
                                            >
                                              <span style={{ fontWeight: 600, fontSize: 13, color: isChecked ? 'var(--text)' : 'inherit' }}>{c.name}</span>
                                              <span style={{ fontSize: 11, color: 'var(--text-secondary)', marginLeft: 8 }}>
                                                {c.provider_type ? `(${c.provider_type})` : ''}
                                              </span>
                                              <Typography.Text keyboard style={{ color: '#1677ff', fontSize: 11, marginLeft: 8 }}>YID: {c.yid || '-'}</Typography.Text>
                                              {isConfigDisabled && (
                                                <Tag color="error" style={{ margin: '0 0 0 8px', fontSize: 11 }}>已禁用</Tag>
                                              )}
                                            </Checkbox>
                                            <Space size={4} align="center">
                                               <Tag color="orange" style={{ margin: 0, fontSize: 11, borderRadius: 4 }}>倍率: {c.rate ?? 1.0}x</Tag>
                                               {isChecked ? (
                                                 <span onClick={(e) => e.stopPropagation()} style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }}>
                                                   <Tooltip title="组内优先级：留空则跟随该上游全局值">
                                                     <span style={{ display: 'inline-flex', alignItems: 'center', gap: 2 }}>
                                                       <span style={{ fontSize: 11, color: 'var(--text-secondary)' }}>P</span>
                                                       <InputNumber
                                                         size="small"
                                                         min={0}
                                                         max={999}
                                                         precision={0}
                                                         controls={false}
                                                         style={{ width: 74 }}
                                                         placeholder={`全局 ${c.priority ?? 0}`}
                                                         value={subPolicyValue(c.id, 'priority')}
                                                         onChange={(v) => setSubPolicyField(c.id, 'priority', v === null ? null : Number(v))}
                                                       />
                                                     </span>
                                                   </Tooltip>
                                                   <Tooltip title="组内权重：同档按权重比例分流；留空则跟随该上游全局值">
                                                     <span style={{ display: 'inline-flex', alignItems: 'center', gap: 2 }}>
                                                       <span style={{ fontSize: 11, color: 'var(--text-secondary)' }}>W</span>
                                                       <InputNumber
                                                         size="small"
                                                         min={0}
                                                         max={100}
                                                         precision={0}
                                                         controls={false}
                                                         style={{ width: 66 }}
                                                         placeholder={`全局 ${c.weight ?? 1}`}
                                                         value={subPolicyValue(c.id, 'weight')}
                                                         onChange={(v) => setSubPolicyField(c.id, 'weight', v === null ? null : Number(v))}
                                                       />
                                                     </span>
                                                   </Tooltip>
                                                 </span>
                                               ) : (
                                                 <>
                                                   <Tag color="blue" style={{ margin: 0, fontSize: 11, borderRadius: 4 }}>优先级: {c.priority ?? 0}</Tag>
                                                   <Tag color="cyan" style={{ margin: 0, fontSize: 11, borderRadius: 4 }}>权重: {c.weight ?? 1}</Tag>
                                                 </>
                                               )}
                                            </Space>
                                          </div>
                                        );
                                      })}
                                    </Space>
                                  </div>
                                )}
                              </>
                            );
                          }

                          let items: any[] = [];
                          let statusCounts = { all: 0, active: 0, disabled: 0 };
                          if (upstreamTab === 'preset') {
                            const searched = presets.filter(p => p.id !== -99 && p.provider_type !== 'high_availability_group' && (
                              p.name?.toLowerCase().includes(presetSearchText.toLowerCase()) || 
                              p.provider_type?.toLowerCase().includes(presetSearchText.toLowerCase()) || 
                              String(p.id).includes(presetSearchText) ||
                              (p.yid && String(p.yid).includes(presetSearchText))
                            ));
                            statusCounts = {
                              all: searched.length,
                              active: searched.filter(p => (p.status ?? 1) === 1).length,
                              disabled: searched.filter(p => (p.status ?? 1) !== 1).length,
                            };
                            items = searched.filter(p =>
                              upstreamStatusFilter === 'all' || (p.status ?? 1) === upstreamStatusFilter
                            );
                          } else if (upstreamTab === 'volcengine_enhance') {
                            items = volcengineEnhanceKeys.filter(p =>
                              p.name?.toLowerCase().includes(presetSearchText.toLowerCase()) ||
                              p.api_key?.toLowerCase().includes(presetSearchText.toLowerCase())
                            );
                          } else if (upstreamTab === 'comfyui') {
                            items = comfyuiServers.filter(p =>
                              p.name?.toLowerCase().includes(presetSearchText.toLowerCase()) ||
                              p.base_url?.toLowerCase().includes(presetSearchText.toLowerCase()) ||
                              String(p.id).includes(presetSearchText)
                            );
                          }

                          return (
                            <>


                              {/* 2. 搜索框 + 状态筛选 */}
                              <div style={{ marginBottom: 10, display: 'flex', gap: 8, flexWrap: 'wrap', alignItems: 'center' }}>
                                <Input
                                  placeholder="输入关键字搜索名称或 ID..."
                                  allowClear
                                  prefix={<SearchOutlined style={{ color: 'var(--text-secondary)' }} />}
                                  value={presetSearchText}
                                  onChange={(e) => setPresetSearchText(e.target.value)}
                                  style={{ flex: 1, minWidth: 200 }}
                                />
                                {upstreamTab === 'preset' && (
                                  <Segmented
                                    size="small"
                                    options={[
                                      { label: `全部 (${statusCounts.all})`, value: 'all' },
                                      { label: `激活 (${statusCounts.active})`, value: '1' },
                                      { label: `禁用 (${statusCounts.disabled})`, value: '0' },
                                    ]}
                                    value={upstreamStatusFilter === 'all' ? 'all' : String(upstreamStatusFilter)}
                                    onChange={(val) => setUpstreamStatusFilter(val === 'all' ? 'all' : parseInt(val as string, 10) as 0 | 1)}
                                  />
                                )}
                              </div>

                              {upstreamTab === 'comfyui' && (
                                <div style={{ marginBottom: 10, display: 'flex', gap: 8, flexWrap: 'wrap', alignItems: 'center' }}>
                                  <Text strong style={{ whiteSpace: 'nowrap' }}>调用规则</Text>
                                  <Select
                                    style={{ minWidth: 220, flex: 1 }}
                                    value={configObj.comfyui_dispatch || COMFYUI_DISPATCH_DEFAULT}
                                    onChange={(v) => setConfigObj((prev) => ({ ...prev, comfyui_dispatch: v }))}
                                    options={comfyuiDispatchRules
                                      .filter((r) => r.is_active === 1)
                                      .map((r) => ({ value: r.code, label: r.name, title: r.remark || undefined }))}
                                    placeholder="选择调用规则"
                                  />
                                  <Text type="secondary" style={{ fontSize: 12 }}>
                                    可多选节点；每次请求按规则选一台
                                  </Text>
                                </div>
                              )}

                              {items.length === 0 ? (
                                <div style={{ padding: '40px 0', textAlign: 'center', color: 'var(--text-secondary)' }}>
                                  {upstreamTab === 'comfyui'
                                    ? '请先在「站点插件 → ComfyUI 接入」中新增服务节点'
                                    : '暂无匹配的上游渠道配置'}
                                </div>
                              ) : (
                                <div style={{ 
                                  maxHeight: 'calc(100vh - 320px)', minHeight: '400px', 
                                  overflowY: 'auto', 
                                  paddingRight: 4 
                                }}>
                                  <Space direction="vertical" style={{ width: '100%' }} size={4}>

                                    {items.map((item) => {
                                      let isSelected = false;
                                      let cardTitle = item.name;
                                      let cardSubtitle = '';
                                      let extraTag = null;
                                      const isConfigDisabled = (upstreamTab === 'preset' && (item.status ?? 1) !== 1)
                                        || (upstreamTab === 'comfyui' && item.is_active !== 1);

                                      if (upstreamTab === 'preset') {
                                        isSelected = currentPreset === item.id;
                                        cardSubtitle = item.yid ? `YID: ${item.yid}` : 'YID: -';
                                        extraTag = (
                                          <Space size={4}>
                                             {isConfigDisabled && <Tag color="error" style={{ margin: 0, fontSize: 11, borderRadius: 4 }}>已禁用</Tag>}
                                             <Tag color="default" style={{ margin: 0, fontSize: 11, borderRadius: 4 }}>{item.provider_type}</Tag>
                                             <Tag color="orange" style={{ margin: 0, fontSize: 11, borderRadius: 4 }}>倍率: {item.rate ?? 1.0}x</Tag>
                                             <Tag color="blue" style={{ margin: 0, fontSize: 11, borderRadius: 4 }}>优先级: {(item as any).priority ?? 0}</Tag>
                                             <Tag color="cyan" style={{ margin: 0, fontSize: 11, borderRadius: 4 }}>权重: {(item as any).weight ?? 1}</Tag>
                                          </Space>
                                        );
                                      } else if (upstreamTab === 'volcengine_enhance') {
                                        isSelected = configObj.volcengine_enhance_credential_id === item.id;
                                        cardSubtitle = `基址: ${item.base_url || '-'}`;
                                        extraTag = <Tag color="blue" style={{ margin: 0, fontSize: 11 }}>火山凭证</Tag>;
                                      } else if (upstreamTab === 'comfyui') {
                                        const selectedIds = parseComfyuiServerIds(configObj, comfyuiWorkflows);
                                        isSelected = selectedIds.includes(item.id);
                                        cardSubtitle = item.base_url ? `基址: ${item.base_url}` : '';
                                        extraTag = (
                                          <Space size={4}>
                                            {item.is_active !== 1 && <Tag color="error" style={{ margin: 0, fontSize: 11, borderRadius: 4 }}>停用</Tag>}
                                            <Tag color="blue" style={{ margin: 0, fontSize: 11, borderRadius: 4 }}>优先级: {item.priority ?? 0}</Tag>
                                            <Tag color="cyan" style={{ margin: 0, fontSize: 11, borderRadius: 4 }}>权重: {item.weight ?? 1}</Tag>
                                            <Tag color="geekblue" style={{ margin: 0, fontSize: 11 }}>服务节点</Tag>
                                          </Space>
                                        );
                                      }

                                      return (

                                          <div
                                            onClick={() => {
                                              if (upstreamTab === 'preset') {
                                                const isHa = item.provider_type === 'high_availability_group';
                                                if (!isHa) {
                                                  setSelectedSubChannelAids([]);
                                                }
                                                form.setFieldsValue({
                                                  preset_id: item.id,
                                                  rate: item.rate ?? 1.0,
                                                  provider_type: item.provider_type || 'custom'
                                                });
                                              } else {
                                                setSelectedSubChannelAids([]);
                                                if (upstreamTab === 'volcengine_enhance') {
                                                  form.setFieldsValue({
                                                    preset_id: null,
                                                    provider_type: 'volcengine',
                                                  });
                                                  setConfigObj(prev => {
                                                    const { comfyui_workflow_id, comfyui_server_id, comfyui_server_ids, comfyui_dispatch, ...rest } = prev;
                                                    return { ...rest, volcengine_enhance_credential_id: item.id };
                                                  });
                                                } else if (upstreamTab === 'comfyui') {
                                                  const selectedIds = parseComfyuiServerIds(configObj, comfyuiWorkflows);
                                                  const next = selectedIds.includes(item.id)
                                                    ? selectedIds.filter((id: number) => id !== item.id)
                                                    : [...selectedIds, item.id];
                                                  form.setFieldsValue({
                                                    preset_id: null,
                                                    provider_type: next.length ? 'comfyui' : 'custom',
                                                  });
                                                  setConfigObj(prev => {
                                                    const { volcengine_enhance_credential_id, comfyui_workflow_id, comfyui_server_id, comfyui_server_ids, ...rest } = prev;
                                                    return {
                                                      ...rest,
                                                      comfyui_server_ids: next,
                                                      comfyui_server_id: next[0],
                                                      comfyui_dispatch: prev.comfyui_dispatch || COMFYUI_DISPATCH_DEFAULT,
                                                    };
                                                  });
                                                }
                                              }
                                            }}
                                            style={{
                                              padding: '6px 10px',
                                              borderRadius: 6,
                                              border: isSelected ? '1px solid #91caff' : (isLight ? '1px solid #e5e4e7' : '1px solid rgba(255,255,255,0.08)'),
                                              background: isSelected ? (isLight ? 'rgba(22,119,255,0.08)' : 'rgba(22,119,255,0.15)') : 'transparent',
                                              cursor: 'pointer',
                                              opacity: isConfigDisabled ? 0.75 : 1,
                                              display: 'flex',
                                              justifyContent: 'space-between',
                                              alignItems: 'center',
                                              transition: 'all 0.15s'
                                            }}
                                          >
                                            <div style={{ flex: 1, display: 'flex', alignItems: 'center', minWidth: 0, gap: 8 }}>
                                              <span style={{ fontWeight: 600, fontSize: 13, color: isSelected ? 'var(--text)' : 'inherit', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                                                {cardTitle}
                                              </span>
                                              {upstreamTab === 'preset' ? (
                                                <Typography.Text keyboard style={{ color: '#1677ff', fontSize: 11, margin: 0 }}>
                                                  {item.yid ? `YID: ${item.yid}` : 'YID: -'}
                                                </Typography.Text>
                                              ) : (
                                                <span style={{ fontSize: 11, color: 'var(--text-secondary)', whiteSpace: 'nowrap' }}>
                                                  {cardSubtitle}
                                                </span>
                                              )}
                                            </div>
                                            <div style={{ flexShrink: 0, display: 'flex', alignItems: 'center' }}>
                                              {extraTag}
                                            </div>
                                          </div>
                                      );
                                    })}
                                  </Space>
                                </div>
                              )}
                            </>
                          );
                        }}
                      </Form.Item>
                    </div>

                    <div style={{ display: activeRightPanel === 'levels' ? 'block' : 'none', animation: 'fadeIn 0.2s' }}>
                        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12, flexWrap: 'wrap', gap: 12 }}>
                          <Space size={16} align="center" wrap>
                            <Title level={4} style={{ margin: 0, whiteSpace: 'nowrap' }}>{isExcludeMode ? '不支持用户等级' : '支持用户等级'}</Title>
                            <Form.Item shouldUpdate={(prev, curr) => prev.level_select !== curr.level_select} noStyle>
                              {() => {
                                const selected = form.getFieldValue('level_select') || [];
                                const allLevelIds = availableUserLevels.map(l => l.id.toString());
                                return (
                                  <Space>
                                    <Button onClick={() => { levelsRef.current = allLevelIds; form.setFieldsValue({ level_select: allLevelIds }); }}>全选</Button>
                                    <Button onClick={() => { levelsRef.current = []; form.setFieldsValue({ level_select: [] }); }} disabled={selected.length === 0}>
                                      清空
                                    </Button>
                                  </Space>
                                );
                              }}
                            </Form.Item>
                          </Space>
                          <Space size={4} align="center">
                            <Text type="secondary" style={{ fontSize: 13, display: screens.xs ? 'none' : 'inline-block' }}>访问控制模式：</Text>
                            <Segmented
                              options={['允许模式', '排除模式']}
                              value={isExcludeMode ? '排除模式' : '允许模式'}
                              onChange={(val) => setIsExcludeMode(val === '排除模式')}
                            />
                          </Space>
                        </div>
                        <div style={{ marginBottom: 24 }}>
                          <Text type="secondary" style={{ fontSize: 13 }}>
                            {isExcludeMode ? '选中排除的等级将【不可】使用该渠道。' : '选中允许的等级才【可以】使用该渠道。'}
                          </Text>
                        </div>
                        
                        <Form.Item name="level_select" style={{ marginBottom: 0 }} hidden>
                          <Select mode="multiple" />
                        </Form.Item>
                        
                        <Form.Item shouldUpdate={(prev, curr) => prev.level_select !== curr.level_select} noStyle>
                          {() => {
                            const selectedLevels = form.getFieldValue('level_select') || [];
                            return (
                              <div style={{ maxHeight: 600, overflowY: 'auto', paddingRight: 8 }}>
                                <Row gutter={[12, 12]}>
                                  {availableUserLevels.map((l) => {
                                    const idStr = l.id.toString();
                                    const isSelected = selectedLevels.includes(idStr);
                                    return (
                                      <Col xs={24} sm={12} lg={12} key={idStr}>
                                        <div 
                                          onClick={() => {
                                            const next = isSelected ? selectedLevels.filter((id: string) => id !== idStr) : [...selectedLevels, idStr];
                                            levelsRef.current = next;
                                            form.setFieldsValue({ level_select: next });
                                          }}
                                          style={{
                                            padding: '8px 12px',
                                            borderRadius: 6,
                                            border: isSelected ? '1px solid var(--text)' : (isLight ? '1px solid #e5e4e7' : '1px solid rgba(255,255,255,0.08)'),
                                            background: isSelected ? (isLight ? 'rgba(0,0,0,0.04)' : 'rgba(255,255,255,0.08)') : 'transparent',
                                            cursor: 'pointer',
                                            display: 'flex',
                                            alignItems: 'center',
                                            transition: 'all 0.2s'
                                          }}
                                        >
                                          <div style={{ flex: 1, minWidth: 0 }}>
                                            <div style={{ fontWeight: 500, fontSize: 14, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', color: isSelected ? 'var(--text)' : 'inherit' }}>{l.name} ({l.discount}x)</div>
                                            <div style={{ fontSize: 12, color: 'var(--text-secondary)', marginTop: 2 }}>ULID: {idStr.padStart(4, '0')} | {l.group_key}</div>
                                          </div>
                                        </div>
                                      </Col>
                                    );
                                  })}
                                </Row>
                              </div>
                            );
                          }}
                        </Form.Item>
                      </div>


                    <div style={{ display: activeRightPanel === 'mapping' ? 'block' : 'none', animation: 'fadeIn 0.2s' }}>
                        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 24 }}>
                          <Title level={4} style={{ margin: 0 }}>模型别名映射</Title>
                          <Switch checked={showMapping} onChange={setShowMapping} />
                        </div>
                        <Text type="secondary" style={{ display: 'block', marginBottom: 24 }}>开启后可为每个模型指定上游别名，解决上下游模型名称不一致的问题。</Text>
                        
                        {showMapping ? (
                          <Form.Item shouldUpdate={(prev, curr) => prev.models !== curr.models || prev.provider_type !== curr.provider_type || prev.category_id !== curr.category_id} noStyle>
                            {() => {
                              const providerType = form.getFieldValue('provider_type');
                              const isHaMode = providerType === 'high_availability_group';
                              // Plugin: happyhorse_router 模型别名映射适配（插件可移除）
                              // 快乐小马模式下基于4个视频模型生成映射列表，而非 routing_node
                              let selectedModels = form.getFieldValue('models') || [];
                              if (isHappyHorseRouting && hhModule) {
                                const activeConfig = happyHorseConfigs.find((c: any) => c.routing_node === selectedRoutingNode);
                                const hhModels = hhModule.getHappyHorseMappingModels(activeConfig || null);
                                selectedModels = hhModels.map((m: any) => m.modelId);
                              }
                              if (selectedModels.length === 0) {
                                return <div style={{ padding: 40, textAlign: 'center', color: 'var(--text)', background: isLight ? '#f9fafb' : 'rgba(255,255,255,0.04)', borderRadius: 8 }}>{isHappyHorseRouting ? '请先选择推理节点' : '请先在左侧选择模型'}</div>;
                              }

                              // HA 模式下获取已绑定的子渠道信息
                              const haSubChannels = isHaMode
                                ? presets.filter((p: any) => selectedSubChannelAids.includes(p.id))
                                : [];
                              const channelCategoryName = resolveCategoryName(form.getFieldValue('category_id'));

                              const toggleResScope = (modelId: string, scope: string) => {
                                const key = resScopeKey(modelId, scope);
                                setExpandedResScopes(prev =>
                                  prev.includes(key) ? prev.filter(k => k !== key) : [...prev, key]
                                );
                              };

                              const updateResAlias = (modelId: string, scope: string, res: string, alias: string) => {
                                const resKey = normalizeResKey(res);
                                if (!resKey) return;
                                setResModelMappingState(prev => {
                                  const map = { ...(prev[modelId]?.[scope] || {}) };
                                  if (!alias.trim()) delete map[resKey];
                                  else map[resKey] = alias;
                                  return patchResScope(prev, modelId, scope, Object.keys(map).length ? map : null);
                                });
                              };

                              const clearResScope = (modelId: string, scope: string) => {
                                setResModelMappingState(prev => patchResScope(prev, modelId, scope, null));
                                setExpandedResScopes(prev => prev.filter(k => k !== resScopeKey(modelId, scope)));
                              };

                              const renderResAdvancedBtn = (modelId: string, scope: string) => {
                                const scopeMap = resModelMappingState[modelId]?.[scope] || {};
                                const scopeKey = resScopeKey(modelId, scope);
                                const isOpen = expandedResScopes.includes(scopeKey);
                                const count = countFilled(scopeMap);
                                return (
                                  <Tooltip title={isOpen ? '收起分辨率映射' : '高级：按分辨率映射不同模型名'}>
                                    <Button
                                      type={count > 0 || isOpen ? 'primary' : 'default'}
                                      size="small"
                                      icon={<SettingOutlined />}
                                      onClick={() => toggleResScope(modelId, scope)}
                                      style={{
                                        height: 24,
                                        padding: '0 8px',
                                        fontSize: 12,
                                        flexShrink: 0,
                                        ...(!(count > 0 || isOpen) ? {
                                          color: 'var(--text-secondary)',
                                          borderColor: isLight ? 'rgba(0,0,0,0.15)' : 'rgba(255,255,255,0.22)',
                                          background: isLight ? '#fff' : 'rgba(255,255,255,0.06)',
                                        } : {}),
                                      }}
                                    >
                                      高级{count > 0 ? ` ${count}` : ''}
                                    </Button>
                                  </Tooltip>
                                );
                              };

                              const renderResAdvancedPanel = (modelId: string, scope: string, presets: string[]) => {
                                const scopeMap = resModelMappingState[modelId]?.[scope] || {};
                                const scopeKey = resScopeKey(modelId, scope);
                                if (!expandedResScopes.includes(scopeKey)) return null;
                                const configuredKeys = Object.keys(scopeMap).filter(k => isFilled(scopeMap[k]));
                                // 预设按模型类型；已配置的历史档位仍展示便于清理
                                const rows = [...new Set([...presets, ...configuredKeys])];
                                return (
                                  <div style={{
                                    marginTop: 6,
                                    marginLeft: 128,
                                    padding: '8px 10px',
                                    borderRadius: 6,
                                    background: isLight ? 'rgba(22,119,255,0.04)' : 'rgba(22,119,255,0.08)',
                                    border: isLight ? '1px solid rgba(22,119,255,0.15)' : '1px solid rgba(105,177,255,0.2)',
                                  }}>
                                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6 }}>
                                      <span style={{ fontSize: 11, color: 'var(--text-secondary)' }}>
                                        分辨率 → 上游模型（未填回退明文别名）
                                      </span>
                                      {configuredKeys.length > 0 && (
                                        <Button type="link" size="small" danger style={{ padding: 0, height: 'auto', fontSize: 11 }} onClick={() => clearResScope(modelId, scope)}>
                                          清空
                                        </Button>
                                      )}
                                    </div>
                                    <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
                                      {rows.map(res => (
                                        <div key={res} style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                                          <Tag style={{ margin: 0, minWidth: 48, textAlign: 'center', fontSize: 11 }}>{res}</Tag>
                                          <Input
                                            size="small"
                                            placeholder={`${res} 上游模型名`}
                                            value={scopeMap[res] || ''}
                                            onChange={(e) => updateResAlias(modelId, scope, res, e.target.value)}
                                            style={{ flex: 1 }}
                                          />
                                          {scopeMap[res] && (
                                            <Button
                                              type="text"
                                              size="small"
                                              icon={<CloseOutlined style={{ fontSize: 10 }} />}
                                              style={{ width: 20, height: 20, minWidth: 20, padding: 0, color: 'var(--text-secondary)' }}
                                              onClick={() => updateResAlias(modelId, scope, res, '')}
                                            />
                                          )}
                                        </div>
                                      ))}
                                    </div>
                                  </div>
                                );
                              };

                              return (
                                <div style={{ maxHeight: 'calc(100vh - 340px)', minHeight: 300, overflowY: 'auto', paddingRight: 12 }}>
                                  {isHaMode && haSubChannels.length > 0 && (
                                    <Alert
                                      message="高可用子渠道独立映射"
                                      description="可为每个子渠道设置独立的模型别名；开启高级设置可按分辨率（视频 480p/…、图片 1k/…）映射不同上游模型名。优先级：子渠该分辨率 → 默认别名该分辨率 → 明文别名（未填档位即回退）。"
                                      type="info"
                                      showIcon
                                      style={{ borderRadius: 6, marginBottom: 16 }}
                                    />
                                  )}
                                  <Row gutter={16}>
                                    {selectedModels.map((midOrId: string) => {
                                      const match = availableModels.find(m => m.mid === midOrId);
                                      const actualModelId = match ? match.model_id : midOrId;
                                      const resPresets = resPresetsForHint(
                                        match?.type_name,
                                        match?.name,
                                        actualModelId,
                                        channelCategoryName,
                                      );
                                      const isActive = activeMappingInputs.includes(actualModelId);
                                      const currentMapping = modelMappingState;
                                      const hasValue = currentMapping[actualModelId] && String(currentMapping[actualModelId]).trim();
                                      const isHaExpanded = expandedHaModels.includes(actualModelId);
                                      const haSubMapping = haModelMappingState[actualModelId] || {};
                                      const haSubMappingCount = countFilled(haSubMapping);

                                      return (
                                        <Col span={24} key={midOrId}>
                                          <div style={{ 
                                            padding: '12px', 
                                            marginBottom: 12, 
                                            borderRadius: 8, 
                                            border: isLight ? '1px solid #e5e4e7' : '1px solid rgba(255,255,255,0.08)',
                                            background: isActive ? (isLight ? 'rgba(0,0,0,0.02)' : 'rgba(255,255,255,0.02)') : 'transparent',
                                            transition: 'all 0.2s'
                                          }}>
                                            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                                              <div style={{ flex: 1, minWidth: 0, paddingRight: 16 }}>
                                                <div style={{ fontWeight: 500, fontSize: 14, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{match?.name || actualModelId}</div>
                                                <div style={{ fontSize: 12, color: 'var(--text-secondary)' }}>ID: {actualModelId} | MID: {midOrId}</div>
                                              </div>
                                              
                                              <Space size={4}>
                                                {!isActive && !(isHaMode && haSubChannels.length > 0) && (
                                                  <Button 
                                                    type={hasValue ? "default" : "dashed"} 
                                                    size="small" 
                                                    icon={<PlusOutlined />}
                                                    onClick={() => setActiveMappingInputs(prev => [...prev, actualModelId])}
                                                    style={{ flexShrink: 0 }}
                                                  >
                                                    {hasValue ? "编辑别名" : "添加映射"}
                                                  </Button>
                                                )}
                                                {isHaMode && haSubChannels.length > 0 && (
                                                  <Button
                                                    type={isHaExpanded ? 'primary' : 'default'}
                                                    size="small"
                                                    icon={<ApartmentOutlined />}
                                                    onClick={() => {
                                                      setExpandedHaModels(prev =>
                                                        prev.includes(actualModelId)
                                                          ? prev.filter(id => id !== actualModelId)
                                                          : [...prev, actualModelId]
                                                      );
                                                    }}
                                                    style={{
                                                      flexShrink: 0,
                                                      ...(!isHaExpanded ? {
                                                        color: 'var(--text)',
                                                        borderColor: isLight ? 'rgba(0,0,0,0.15)' : 'rgba(255,255,255,0.22)',
                                                        background: isLight ? '#fff' : 'rgba(255,255,255,0.06)',
                                                      } : {}),
                                                    }}
                                                  >
                                                    {isHaExpanded ? '收起' : '子渠道映射'}{haSubMappingCount > 0 ? ` (${haSubMappingCount})` : ''}
                                                  </Button>
                                                )}
                                              </Space>
                                            </div>

                                            {/* 默认别名输入 */}
                                            {isActive && !(isHaMode && haSubChannels.length > 0) && (
                                              <div style={{ marginTop: 12, display: 'flex', gap: 8, animation: 'fadeIn 0.2s' }}>
                                                <div style={{ marginBottom: 0, flex: 1 }}>
                                                  <Input 
                                                    placeholder={`请输入上游调用的实际名称，不填则默认：${actualModelId}`} 
                                                    autoFocus 
                                                    value={modelMappingState[actualModelId] || ''}
                                                    onChange={(e) => {
                                                      const val = e.target.value;
                                                      setModelMappingState(prev => ({ ...prev, [actualModelId]: val }));
                                                    }}
                                                    addonBefore={isHaMode ? <span style={{ fontSize: 11, color: 'var(--text-secondary)' }}>默认</span> : undefined}
                                                  />
                                                </div>
                                                <Button 
                                                  icon={<DeleteOutlined />} 
                                                  onClick={() => {
                                                    setActiveMappingInputs(prev => prev.filter(id => id !== actualModelId));
                                                    setModelMappingState(prev => {
                                                      const next = { ...prev };
                                                      delete next[actualModelId];
                                                      return next;
                                                    });
                                                  }}
                                                />
                                              </div>
                                            )}

                                            {/* HA 子渠道独立映射 */}
                                            {isHaMode && isHaExpanded && haSubChannels.length > 0 && (
                                              <div style={{ 
                                                marginTop: 12, 
                                                padding: '10px 12px', 
                                                background: isLight ? 'rgba(0,0,0,0.02)' : 'rgba(255,255,255,0.02)', 
                                                borderRadius: 6, 
                                                border: isLight ? '1px dashed #d9d9d9' : '1px dashed rgba(255,255,255,0.12)',
                                                animation: 'fadeIn 0.2s'
                                              }}>
                                                <div style={{ marginBottom: 8, fontSize: 12, color: 'var(--text-secondary)', fontWeight: 500 }}>
                                                  <ApartmentOutlined style={{ marginRight: 4 }} />
                                                  子渠道独立映射（覆盖默认别名）
                                                </div>
                                                <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                                                  {/* 默认别名 (回退) */}
                                                  <div style={{ 
                                                    paddingBottom: 6, 
                                                    borderBottom: isLight ? '1px dashed #f0f0f0' : '1px dashed rgba(255,255,255,0.06)',
                                                    marginBottom: 4 
                                                  }}>
                                                    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                                                      <div style={{ 
                                                        minWidth: 120, 
                                                        maxWidth: 160, 
                                                        flexShrink: 0,
                                                        fontSize: 12,
                                                        fontWeight: 600,
                                                        color: 'var(--text-secondary)',
                                                      }}>
                                                        <GlobalOutlined style={{ marginRight: 4, color: '#fa8c16' }} />
                                                        默认别名 (回退)
                                                      </div>
                                                      <Input
                                                        size="small"
                                                        placeholder={actualModelId}
                                                        value={modelMappingState[actualModelId] || ''}
                                                        onChange={(e) => {
                                                          const val = e.target.value;
                                                          setModelMappingState(prev => {
                                                            if (!val.trim()) {
                                                              const next = { ...prev };
                                                              delete next[actualModelId];
                                                              return next;
                                                            }
                                                            return { ...prev, [actualModelId]: val };
                                                          });
                                                        }}
                                                        style={{ flex: 1 }}
                                                      />
                                                      {renderResAdvancedBtn(actualModelId, 'default')}
                                                      {modelMappingState[actualModelId] && (
                                                        <Button
                                                          type="text"
                                                          size="small"
                                                          icon={<CloseOutlined style={{ fontSize: 10 }} />}
                                                          style={{ width: 20, height: 20, minWidth: 20, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-secondary)', margin: 0, padding: 0 }}
                                                          onClick={() => {
                                                            setModelMappingState(prev => {
                                                              const next = { ...prev };
                                                              delete next[actualModelId];
                                                              return next;
                                                            });
                                                          }}
                                                        />
                                                      )}
                                                    </div>
                                                    {renderResAdvancedPanel(actualModelId, 'default', resPresets)}
                                                  </div>
                                                  {haSubChannels.map((sub: any) => {
                                                    const subIdStr = String(sub.id);
                                                    const subAlias = haSubMapping[subIdStr] || '';
                                                    return (
                                                      <div key={sub.id}>
                                                        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                                                          <div style={{ 
                                                            minWidth: 120, 
                                                            maxWidth: 160, 
                                                            flexShrink: 0,
                                                            fontSize: 12,
                                                            fontWeight: 500,
                                                            overflow: 'hidden',
                                                            textOverflow: 'ellipsis',
                                                            whiteSpace: 'nowrap'
                                                          }} title={`${sub.name} (YID: ${sub.yid || '-'})`}>
                                                            <CloudServerOutlined style={{ marginRight: 4, color: '#1677ff' }} />
                                                            {sub.name}
                                                          </div>
                                                          <Input
                                                            size="small"
                                                            placeholder={modelMappingState[actualModelId] || actualModelId}
                                                            value={subAlias}
                                                            onChange={(e) => {
                                                              const val = e.target.value;
                                                              setHaModelMappingState(prev => ({
                                                                ...prev,
                                                                [actualModelId]: {
                                                                  ...(prev[actualModelId] || {}),
                                                                  [subIdStr]: val,
                                                                }
                                                              }));
                                                            }}
                                                            style={{ flex: 1 }}
                                                          />
                                                          {renderResAdvancedBtn(actualModelId, subIdStr)}
                                                          {subAlias && (
                                                            <Button
                                                              type="text"
                                                              size="small"
                                                              icon={<CloseOutlined style={{ fontSize: 10 }} />}
                                                              style={{ width: 20, height: 20, minWidth: 20, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-secondary)', margin: 0, padding: 0 }}
                                                              onClick={() => {
                                                                setHaModelMappingState(prev => {
                                                                  const next = { ...prev };
                                                                  const subMap = { ...(next[actualModelId] || {}) };
                                                                  delete subMap[subIdStr];
                                                                  if (Object.keys(subMap).length === 0) {
                                                                    delete next[actualModelId];
                                                                  } else {
                                                                    next[actualModelId] = subMap;
                                                                  }
                                                                  return next;
                                                                });
                                                              }}
                                                            />
                                                          )}
                                                        </div>
                                                        {renderResAdvancedPanel(actualModelId, subIdStr, resPresets)}
                                                      </div>
                                                    );
                                                  })}
                                                </div>
                                              </div>
                                            )}
                                          </div>
                                        </Col>
                                      );
                                    })}
                                  </Row>
                                </div>
                              );
                            }}
                          </Form.Item>
                        ) : (
                          <div style={{ padding: 40, textAlign: 'center', color: 'var(--text)', background: isLight ? '#f9fafb' : 'rgba(255,255,255,0.04)', borderRadius: 8 }}>别名映射功能已关闭</div>
                        )}
                      </div>

                  </div>
                </Col>
              </Row>

              <div style={{ marginTop: 24, paddingTop: 24, borderTop: isLight ? '1px solid #e5e4e7' : '1px solid rgba(255,255,255,0.08)', display: 'flex', justifyContent: 'flex-end', gap: 12 }}>
                <Button size="large" onClick={() => handleCloseModal()}>取消</Button>
                <Button size="large" type="primary" htmlType="submit" loading={submitting} style={{ minWidth: 120 }}>保存设置</Button>
              </div>
            </Form>
          </Spin>
        </div>
        </div>
      )}

      <ChannelCategoryManager
        visible={isCategoryManagerVisible}
        onClose={() => setIsCategoryManagerVisible(false)}
        onUpdate={fetchCategories}
      />

    </Card>
  );
};

export default Channels;
