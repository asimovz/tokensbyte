/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia 
 * @license        MIT (https://www.tokensbyte.ai/)
 */

import React, { useState, useEffect, useMemo, useRef } from 'react';
import {
  Row, Col, Tree, Input, Button, Switch, InputNumber,
  Form, Space, Empty, message, Popconfirm, Tooltip, Modal, Tag, Spin
} from 'antd';
import {
  PlusOutlined, DeleteOutlined, SaveOutlined, ReloadOutlined,
  FolderOutlined, FileTextOutlined, ArrowUpOutlined, ArrowDownOutlined,
  SettingOutlined, SearchOutlined, FolderAddOutlined, FileAddOutlined
} from '@ant-design/icons';
import { MdEditor } from 'md-editor-rt';
import 'md-editor-rt/lib/style.css';
import './DocsApi.css';
import request from '../../../utils/request';
import { useThemeStore } from '../../../store/theme';
import { useTranslation } from 'react-i18next';

interface DocNode {
  id: number;
  parent_id: number | null;
  title: string;
  is_dir: boolean;
  sort_order: number;
  is_active: boolean;
  slug?: string;
  category_id?: number | null;
  children: DocNode[];
}

interface DocDetail {
  id: number;
  parent_id: number | null;
  title: string;
  content: string;
  is_dir: number;
  sort_order: number;
  is_active: number;
  created_at: string;
  updated_at: string;
  slug?: string;
  translations?: Record<string, { title: string; content?: string }>;
  category_id?: number | null;
}

interface DocCategory {
  id: number;
  name: string;
  sort_order: number;
  is_default?: number;
}

interface DocsManagerProps {
  apiPrefix?: string;
}

const DocsManager: React.FC<DocsManagerProps> = ({ apiPrefix = '/plugins/docs-api' }) => {
  const { t } = useTranslation('docs_api');
  const { themeMode } = useThemeStore();
  const isLight = themeMode === 'light';

  const LANGUAGES = useMemo(() => [
    { key: 'zh', label: t('lang_zh_default') },
    { key: 'zh-TW', label: '繁體中文' },
    { key: 'en', label: 'English' },
    { key: 'ja', label: '日本語' },
    { key: 'ko', label: '한국어' },
    { key: 'vi', label: 'Tiếng Việt' },
  ], [t]);

  const editorRef = useRef<any>(null);

  const [treeData, setTreeData] = useState<DocNode[]>([]);
  const [flatDocs, setFlatDocs] = useState<any[]>([]);
  const [loading, setLoading] = useState(false);
  const [selectedKey, setSelectedKey] = useState<number | null>(null);
  const [editingDoc, setEditingDoc] = useState<DocDetail | null>(null);
  const [activeLang, setActiveLang] = useState<'zh' | 'zh-TW' | 'en' | 'ja' | 'ko' | 'vi'>('zh');
  const [translating, setTranslating] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const [expandedKeys, setExpandedKeys] = useState<React.Key[]>([]);
  const [detailLoading, setDetailLoading] = useState(false);
  const selectReqSeqRef = useRef(0);

  // 模态框：创建新节点
  const [createModalVisible, setCreateModalVisible] = useState(false);
  const [createForm] = Form.useForm();
  const [createParentId, setCreateParentId] = useState<number | null>(null);
  const [createIsDir, setCreateIsDir] = useState<number>(0);

  // 分类管理
  const isSitePortalPro = false;
  const [categories, setCategories] = useState<DocCategory[]>([]);
  const [activeCategoryId, setActiveCategoryId] = useState<number | null>(null);
  const [categoryModalVisible, setCategoryModalVisible] = useState(false);
  const [categoryForm] = Form.useForm();
  const [editingCategory, setEditingCategory] = useState<DocCategory | null>(null);

  // Shadcn 调色板
  const colors = {
    background: isLight ? '#ffffff' : '#09090b',
    foreground: isLight ? '#09090b' : '#f4f4f5',
    card: isLight ? '#ffffff' : '#09090b',
    cardMuted: isLight ? '#f4f4f5' : '#18181b',
    border: isLight ? '#e4e4e7' : '#27272a',
    input: isLight ? '#ffffff' : '#09090b',
    muted: isLight ? '#71717a' : '#a1a1aa',
    accent: isLight ? '#f4f5f6' : '#1a1a1c',
    primary: isLight ? '#18181b' : '#ffffff',
    primaryText: isLight ? '#ffffff' : '#09090b',
    ring: isLight ? '#cbd5e1' : '#3f3f46',
  };

  const styleSheet = {
    container: {
      display: 'flex',
      flexDirection: 'column' as const,
      gap: '12px',
      height: 'calc(100vh - 160px)',
      color: colors.foreground,
      fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif',
    },
    body: {
      display: 'flex',
      gap: '24px',
      flex: 1,
      minHeight: 0,
    },
    sidebar: {
      width: '320px',
      minWidth: '320px',
      display: 'flex',
      flexDirection: 'column' as const,
      border: `1px solid ${colors.border}`,
      borderRadius: '8px',
      background: colors.card,
      padding: '12px 8px',
      height: '100%',
    },
    main: {
      flex: 1,
      display: 'flex',
      flexDirection: 'column' as const,
      border: `1px solid ${colors.border}`,
      borderRadius: '8px',
      background: colors.card,
      height: '100%',
      overflow: 'hidden',
    },
    input: {
      height: '36px',
      borderRadius: '6px',
      border: `1px solid ${colors.border}`,
      background: colors.input,
      color: colors.foreground,
      padding: '0 12px',
      fontSize: '14px',
      outline: 'none',
      width: '100%',
      transition: 'border-color 0.2s, box-shadow 0.2s',
    },
    buttonOutline: {
      height: '32px',
      borderRadius: '6px',
      border: `1px solid ${colors.border}`,
      background: 'transparent',
      color: colors.foreground,
      fontSize: '13px',
      fontWeight: 500,
      cursor: 'pointer',
      display: 'inline-flex',
      alignItems: 'center',
      justifyContent: 'center',
      gap: '6px',
      padding: '0 12px',
      transition: 'background 0.2s',
    },
    buttonPrimary: {
      height: '36px',
      borderRadius: '6px',
      background: colors.primary,
      color: colors.primaryText,
      border: 'none',
      fontSize: '14px',
      fontWeight: 500,
      cursor: 'pointer',
      display: 'inline-flex',
      alignItems: 'center',
      justifyContent: 'center',
      gap: '8px',
      padding: '0 16px',
      boxShadow: '0 1px 2px rgba(0,0,0,0.05)',
      transition: 'opacity 0.2s',
    },
    header: {
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'space-between',
      padding: '16px 20px',
      borderBottom: `1px solid ${colors.border}`,
    },
    metaRow: {
      display: 'flex',
      alignItems: 'center',
      gap: '16px',
      padding: '12px 20px',
      background: colors.cardMuted,
      borderBottom: `1px solid ${colors.border}`,
      flexWrap: 'wrap' as const,
    },
    metaItem: {
      display: 'flex',
      alignItems: 'center',
      gap: '8px',
    },
    label: {
      fontSize: '13px',
      fontWeight: 500,
      color: colors.muted,
    },
    select: {
      height: '32px',
      borderRadius: '6px',
      border: `1px solid ${colors.border}`,
      background: colors.input,
      color: colors.foreground,
      padding: '0 8px',
      fontSize: '13px',
      outline: 'none',
      cursor: 'pointer',
    },
    editorWrapper: {
      flex: 1,
      display: 'flex',
      flexDirection: 'column' as const,
      height: 'calc(100% - 110px)',
      background: colors.background,
      overflow: 'hidden',
    }
  };

  useEffect(() => {
    fetchDocs();
    if (isSitePortalPro) {
      fetchCategories();
    }
  }, []);

  const dedupeCategories = (list: DocCategory[]) => {
    const seen = new Set<string>();
    return [...list]
      .sort((a, b) => a.id - b.id)
      .filter(c => {
        if (seen.has(c.name)) return false;
        seen.add(c.name);
        return true;
      })
      .sort((a, b) => a.sort_order - b.sort_order || a.id - b.id);
  };

  const fetchCategories = async () => {
    try {
      const res = await (request.get(`${apiPrefix}/docs/categories`) as any);
      if (res.categories) {
        const cats = dedupeCategories(res.categories);
        setCategories(cats);
        setActiveCategoryId(prev => {
          if (prev !== null && cats.some(c => c.id === prev)) {
            return prev;
          }
          const def = cats.find(c => c.is_default === 1);
          return def?.id ?? cats[0]?.id ?? null;
        });
      }
    } catch (error) {
      console.error(error);
    }
  };

  // 未设置 category_id 的旧文档，归入默认分类（优先 is_default，其次 API 参考）
  const defaultCategoryId = useMemo(() => {
    const def = categories.find(c => c.is_default === 1);
    if (def) return def.id;
    const apiRef = categories.find(c => c.name === 'API 参考' || c.name === 'API参考');
    return apiRef?.id ?? categories[0]?.id ?? null;
  }, [categories]);

  /** 收集某节点的全部子孙 id（用于禁止挂到自身子树） */
  const collectDescendantIds = (rootId: number): Set<number> => {
    const result = new Set<number>();
    const walk = (pid: number) => {
      flatDocs.forEach(d => {
        if (d.parent_id === pid && !result.has(d.id)) {
          result.add(d.id);
          walk(d.id);
        }
      });
    };
    walk(rootId);
    return result;
  };

  /** 解析文档所属分类：自身 category_id → 沿父级向上 → 默认分类 */
  const resolveDocCategoryId = (docId: number | null | undefined): number | null => {
    if (docId == null) return null;
    const visited = new Set<number>();
    let currentId: number | null = docId;
    while (currentId != null && !visited.has(currentId)) {
      visited.add(currentId);
      const doc = flatDocs.find(d => d.id === currentId);
      if (!doc) break;
      if (doc.category_id != null && doc.category_id !== undefined) {
        return doc.category_id;
      }
      currentId = doc.parent_id ?? null;
    }
    return defaultCategoryId;
  };

  /** 切换顶部分类时，若当前选中文档不在该分类下则清空编辑区 */
  const handleAdminCategoryChange = (categoryId: number) => {
    setActiveCategoryId(categoryId);
    if (selectedKey == null) return;
    const selectedCat = resolveDocCategoryId(selectedKey);
    const belongs =
      selectedCat === categoryId ||
      ((selectedCat == null || selectedCat === undefined) && categoryId === defaultCategoryId);
    if (!belongs) {
      selectReqSeqRef.current += 1;
      setSelectedKey(null);
      setEditingDoc(null);
    }
  };

  // 载入或切换文档时，默认开启单独预览模式
  useEffect(() => {
    if (editingDoc && editingDoc.is_dir === 0) {
      const timer = setTimeout(() => {
        if (editorRef.current) {
          editorRef.current.togglePreviewOnly(true);
        }
      }, 50);
      return () => clearTimeout(timer);
    }
  }, [editingDoc?.id]);

  const fetchDocs = async () => {
    try {
      setLoading(true);
      const res = await (request.get(`${apiPrefix}/docs`) as any);
      if (res.tree) {
        setTreeData(res.tree);
        const flat: any[] = [];
        const traverse = (nodes: DocNode[]) => {
          nodes.forEach(n => {
            flat.push({
              id: n.id,
              parent_id: n.parent_id,
              title: n.title,
              is_dir: n.is_dir,
              sort_order: n.sort_order,
              is_active: n.is_active,
              slug: n.slug,
              category_id: n.category_id,
            });
            if (n.children) traverse(n.children);
          });
        };
        traverse(res.tree);
        setFlatDocs(flat);

        if (expandedKeys.length === 0) {
          setExpandedKeys(res.tree.map((n: any) => String(n.id)));
        }
      }
    } catch (error) {
      message.error(t('msg_fetch_list_failed'));
    } finally {
      setLoading(false);
    }
  };

  const handleSelect = async (keys: any[]) => {
    if (keys.length === 0) {
      selectReqSeqRef.current += 1;
      setSelectedKey(null);
      setEditingDoc(null);
      setDetailLoading(false);
      return;
    }
    const docId = Number(keys[0]);
    const reqSeq = ++selectReqSeqRef.current;
    setSelectedKey(docId);
    // 先清空，避免切换时标题/内容残留上一篇
    setEditingDoc(null);
    setActiveLang('zh');
    setDetailLoading(true);
    try {
      const res = await (request.get(`${apiPrefix}/docs/${docId}`) as any);
      // 快速连点时丢弃过期响应，避免旧文档盖住新文档
      if (reqSeq !== selectReqSeqRef.current) return;
      if (res.doc) {
        const doc = res.doc;
        setEditingDoc({
          ...doc,
          category_id:
            doc.category_id != null && doc.category_id !== undefined
              ? doc.category_id
              : (isSitePortalPro ? resolveDocCategoryId(doc.parent_id) : doc.category_id),
        });
      }
    } catch (error) {
      if (reqSeq !== selectReqSeqRef.current) return;
      message.error(t('msg_fetch_detail_failed'));
    } finally {
      if (reqSeq === selectReqSeqRef.current) {
        setDetailLoading(false);
      }
    }
  };

  const handleSave = async () => {
    if (!editingDoc) return;
    try {
      const resolvedCategoryId = isSitePortalPro
        ? (editingDoc.category_id ?? resolveDocCategoryId(editingDoc.parent_id) ?? null)
        : (editingDoc.category_id || null);
      const payload: Record<string, unknown> = {
        parent_id: editingDoc.parent_id,
        title: editingDoc.title,
        content: editingDoc.content,
        sort_order: editingDoc.sort_order,
        is_active: editingDoc.is_active,
        slug: editingDoc.slug || '',
        // 顶级节点写入分类；有父级时跟随父级所属分类
        category_id: editingDoc.parent_id ? null : resolvedCategoryId,
      };
      // 仅在已加载到翻译时提交，避免传 {} 触发后端清空 intl
      if (editingDoc.translations) {
        payload.translations = editingDoc.translations;
      }
      await request.put(`${apiPrefix}/docs/${editingDoc.id}`, payload);
      message.success(t('msg_save_success'));
      fetchDocs();
    } catch (error) {
      message.error(t('msg_save_failed'));
    }
  };

  const handleAiTranslateAll = async () => {
    if (!editingDoc) return;
    const sourceDocId = editingDoc.id;
    const sourceTitle = editingDoc.title;
    const sourceContent = editingDoc.content;
    const isArticle = editingDoc.is_dir === 0;

    if (!sourceTitle && !sourceContent) {
      message.warning(t('translate_warning'));
      return;
    }

    setTranslating(true);
    message.loading({ content: t('translate_loading'), key: 'translate-status', duration: 0 });

    try {
      const targetLangs = ['zh-TW', 'en', 'ja', 'ko', 'vi'];
      const newTranslations: Record<string, { title: string; content?: string }> = {
        ...(editingDoc.translations || {}),
      };

      for (const lang of targetLangs) {
        let translatedTitle = '';
        let translatedContent = '';

        if (sourceTitle) {
          const resTitle = await request.post(`${apiPrefix}/docs/translate`, {
            text: sourceTitle,
            to_lang: lang,
          }) as any;
          translatedTitle = resTitle.translated || '';
        }

        if (sourceContent && isArticle) {
          const resContent = await request.post(`${apiPrefix}/docs/translate`, {
            text: sourceContent,
            to_lang: lang,
          }) as any;
          translatedContent = resContent.translated || '';
        }

        newTranslations[lang] = {
          title: translatedTitle || newTranslations[lang]?.title || '',
          content: translatedContent || newTranslations[lang]?.content || '',
        };
      }

      setEditingDoc(prev => {
        if (!prev || prev.id !== sourceDocId) return prev;
        // 合并写入，保留翻译期间用户对其他语言的手动修改
        return {
          ...prev,
          translations: {
            ...(prev.translations || {}),
            ...newTranslations,
          },
        };
      });

      message.success({ content: t('translate_success'), key: 'translate-status', duration: 3 });
    } catch (error: any) {
      message.error({ content: `${t('translate_failed')}: ${error?.message || 'unknown error'}`, key: 'translate-status', duration: 3 });
    } finally {
      setTranslating(false);
    }
  };

  const handleDelete = async (id: number) => {
    try {
      await request.delete(`${apiPrefix}/docs/${id}`);
      message.success(t('msg_delete_success'));
      if (selectedKey === id) {
        setSelectedKey(null);
        setEditingDoc(null);
      }
      fetchDocs();
    } catch (error) {
      message.error(t('msg_delete_failed'));
    }
  };

  const handleResetInitializeDocs = async () => {
    try {
      setLoading(true);
      await request.post(`${apiPrefix}/docs/import-default`);
      message.success(t('msg_reset_success'));
      setSelectedKey(null);
      setEditingDoc(null);
      setSearchQuery('');
      await fetchDocs();
      if (isSitePortalPro) {
        await fetchCategories();
      }
    } catch (error) {
      message.error(t('msg_reset_failed'));
    } finally {
      setLoading(false);
    }
  };

  const handleClearAllDocs = async () => {
    try {
      setLoading(true);
      await request.post(`${apiPrefix}/docs/clear-all`);
      message.success(t('clear_success', '已成功清空所有数据'));
      setSelectedKey(null);
      setEditingDoc(null);
      setSearchQuery('');
      await fetchDocs();
      if (isSitePortalPro) {
        await fetchCategories();
      }
    } catch (error: any) {
      const status = error?.response?.status;
      const serverMsg = error?.response?.data?.error?.message || error?.response?.data?.message;
      if (status === 404 || status === 405 || status === 400) {
        message.error('清空接口不可用，请重启后端后再试');
      } else {
        message.error(serverMsg || t('clear_failed', '清空数据失败'));
      }
    } finally {
      setLoading(false);
    }
  };

  const openCreateModal = (parentId: number | null, isDir: number) => {
    setCreateParentId(parentId);
    setCreateIsDir(isDir);
    createForm.resetFields();
    const inheritedCat = parentId != null
      ? resolveDocCategoryId(parentId)
      : (activeCategoryId || undefined);
    createForm.setFieldsValue({
      is_dir: isDir,
      sort_order: 10,
      is_active: true,
      category_id: inheritedCat ?? activeCategoryId ?? undefined,
    });
    setCreateModalVisible(true);
  };

  const handleCreateSubmit = async () => {
    try {
      const values = await createForm.validateFields();
      // 未选分类时保持 null，不再静默回落到当前 Tab
      const categoryId = createParentId
        ? null
        : (values.category_id != null && values.category_id !== ''
          ? Number(values.category_id)
          : null);
      await request.post(`${apiPrefix}/docs`, {
        parent_id: createParentId,
        title: values.title,
        content: values.content || '',
        is_dir: createIsDir,
        sort_order: values.sort_order,
        is_active: values.is_active ? 1 : 0,
        slug: values.slug || '',
        category_id: categoryId,
      });
      message.success(t('msg_add_success'));
      setCreateModalVisible(false);
      fetchDocs();
    } catch (error: any) {
      if (error?.errorFields) return; // 表单校验失败，不提示「添加失败」
      message.error(t('msg_add_failed'));
    }
  };

  const onDrop = async (info: any) => {
    const dragId = Number(info.dragNode.key);
    const dropId = Number(info.node.key);
    const dropPos = info.node.pos.split('-');
    const dropPosition = info.dropPosition - Number(dropPos[dropPos.length - 1]);

    const dragNode = flatDocs.find(d => d.id === dragId);
    const dropNode = flatDocs.find(d => d.id === dropId);

    if (!dragNode || !dropNode) return;

    // 禁止拖入自身子孙节点，避免目录环
    const descendants = collectDescendantIds(dragId);
    if (dropId === dragId || descendants.has(dropId)) {
      message.warning('不能将节点移动到自身或其子节点下');
      return;
    }

    let nextParentId: number | null = dragNode.parent_id;
    if (dropPosition === 0) {
      if (!dropNode.is_dir) {
        message.warning(t('msg_drag_not_dir'));
        return;
      }
      nextParentId = dropId;
    } else {
      nextParentId = dropNode.parent_id;
    }

    const siblings = flatDocs
      .filter(d => d.parent_id === nextParentId && d.id !== dragId)
      .sort((a, b) => a.sort_order - b.sort_order || a.id - b.id);

    let insertAt = siblings.findIndex(s => s.id === dropId);
    if (insertAt < 0) insertAt = siblings.length;
    if (dropPosition === 0) {
      insertAt = siblings.length;
    } else if (dropPosition > 0) {
      insertAt += 1;
    }

    let nextSortOrder = (insertAt + 1) * 10;
    if (siblings.length > 0) {
      if (insertAt <= 0) {
        nextSortOrder = siblings[0].sort_order - 10;
      } else if (insertAt >= siblings.length) {
        nextSortOrder = siblings[siblings.length - 1].sort_order + 10;
      } else {
        const low = siblings[insertAt - 1].sort_order;
        const high = siblings[insertAt].sort_order;
        nextSortOrder = high - low > 1 ? Math.floor((low + high) / 2) : low + 1;
      }
    }

    try {
      const detailRes = await (request.get(`${apiPrefix}/docs/${dragId}`) as any);
      const detail = detailRes.doc;
      const content = detail?.content || '';
      const slug = detail?.slug || '';
      const nextCategoryId = nextParentId == null
        ? (detail?.category_id ?? dragNode.category_id ?? resolveDocCategoryId(dragId) ?? activeCategoryId ?? null)
        : null;

      await request.put(`${apiPrefix}/docs/${dragId}`, {
        parent_id: nextParentId,
        title: dragNode.title,
        content,
        sort_order: nextSortOrder,
        is_active: dragNode.is_active ? 1 : 0,
        slug,
        category_id: nextCategoryId,
      });

      // 排序间隙不足时，把同级重排为 10/20/30…（仅写 sort_order+原字段）
      if (siblings.length > 0 && insertAt > 0 && insertAt < siblings.length) {
        const low = siblings[insertAt - 1].sort_order;
        const high = siblings[insertAt].sort_order;
        if (high - low <= 1) {
          const ordered = [
            ...siblings.slice(0, insertAt),
            { ...dragNode, id: dragId },
            ...siblings.slice(insertAt),
          ];
          for (let i = 0; i < ordered.length; i++) {
            const node = ordered[i];
            const isDragged = node.id === dragId;
            if (isDragged) {
              await request.put(`${apiPrefix}/docs/${node.id}`, {
                parent_id: nextParentId,
                title: dragNode.title,
                content,
                sort_order: (i + 1) * 10,
                is_active: dragNode.is_active ? 1 : 0,
                slug,
                category_id: nextCategoryId,
              });
              continue;
            }
            const sibDetail = await (request.get(`${apiPrefix}/docs/${node.id}`) as any);
            await request.put(`${apiPrefix}/docs/${node.id}`, {
              parent_id: sibDetail.doc?.parent_id ?? node.parent_id,
              title: node.title,
              content: sibDetail.doc?.content || '',
              sort_order: (i + 1) * 10,
              is_active: node.is_active ? 1 : 0,
              slug: sibDetail.doc?.slug || node.slug || '',
              category_id: sibDetail.doc?.category_id ?? node.category_id ?? null,
            });
          }
        }
      }

      message.success(t('msg_drag_success'));
      if (editingDoc?.id === dragId) {
        setEditingDoc(prev => prev ? {
          ...prev,
          parent_id: nextParentId,
          sort_order: nextSortOrder,
          category_id: nextCategoryId,
        } : prev);
      }
      fetchDocs();
    } catch (error) {
      message.error(t('msg_drag_failed'));
    }
  };

  const renderTreeNodes = (data: DocNode[]): any[] => {
    return data
      .map(item => {
        const titleMatch = item.title.toLowerCase().includes(searchQuery.toLowerCase());
        const hasChildrenMatch = item.children && renderTreeNodes(item.children).length > 0;

        if (searchQuery && !titleMatch && !hasChildrenMatch) {
          return null;
        }

        const icon = item.is_dir 
          ? <FolderOutlined style={{ color: colors.muted, fontSize: '16px' }} /> 
          : <FileTextOutlined style={{ color: colors.muted, fontSize: '16px' }} />;

        const titleNode = (
          <div style={{ display: 'flex', alignItems: 'center', gap: '6px', width: '100%', minWidth: 0 }} className="tree-node-row">
            <span style={{ display: 'inline-flex', alignItems: 'center', flexShrink: 0 }}>
              {icon}
            </span>
            <span style={{ 
              fontSize: '13.5px',
              textDecoration: item.is_active ? 'none' : 'line-through', 
              opacity: item.is_active ? 1 : 0.4,
              whiteSpace: 'nowrap',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              flex: 1,
              minWidth: 0,
              color: colors.foreground
            }} title={item.title}>
              {item.title}
              {item.slug && (
                <span style={{ fontSize: '11px', color: colors.muted, marginLeft: '6px', fontStyle: 'italic', fontWeight: 'normal' }}>
                  ({item.slug})
                </span>
              )}
            </span>
            <Space size={2} className="tree-actions-btn" onClick={e => e.stopPropagation()}>
              {item.is_dir && (
                <Tooltip title={t('new_subdoc')}>
                  <Button
                    type="text"
                    size="small"
                    icon={<FileAddOutlined style={{ fontSize: '11px', color: colors.muted }} />}
                    onClick={(e) => { e.stopPropagation(); openCreateModal(item.id, 0); }}
                    style={{ width: '20px', height: '20px', display: 'flex', alignItems: 'center', justifyContent: 'center', borderRadius: '4px' }}
                  />
                </Tooltip>
              )}
              <Popconfirm
                title={t('delete_confirm_title')}
                onConfirm={(e) => { e?.stopPropagation(); handleDelete(item.id); }}
                onCancel={(e) => e?.stopPropagation()}
                okText={t('delete_btn')}
                cancelText={t('cancel_btn')}
              >
                <Button
                  type="text"
                  size="small"
                  icon={<DeleteOutlined style={{ fontSize: '11px', color: '#ef4444' }} />}
                  onClick={(e) => e.stopPropagation()}
                  style={{ width: '20px', height: '20px', display: 'flex', alignItems: 'center', justifyContent: 'center', borderRadius: '4px' }}
                />
              </Popconfirm>
            </Space>
          </div>
        );

        return {
          key: String(item.id),
          title: titleNode,
          children: item.children ? renderTreeNodes(item.children) : [],
        };
      })
      .filter(item => item !== null);
  };

  const displayTreeData = useMemo(() => {
    if (!isSitePortalPro || activeCategoryId === null) return treeData;
    return treeData.filter(node =>
      node.category_id === activeCategoryId ||
      ((node.category_id == null || node.category_id === undefined) &&
        activeCategoryId === defaultCategoryId)
    );
  }, [treeData, isSitePortalPro, activeCategoryId, defaultCategoryId]);

  /** 上级目录候选项：排除自身子孙；门户增强下再按当前分类过滤 */
  const parentDirOptions = useMemo(() => {
    if (!editingDoc) return [];
    const blocked = collectDescendantIds(editingDoc.id);
    blocked.add(editingDoc.id);
    let dirs = flatDocs.filter(d => d.is_dir && !blocked.has(d.id));
    if (!isSitePortalPro) return dirs;

    const selectedCat =
      editingDoc.category_id != null && editingDoc.category_id !== undefined
        ? editingDoc.category_id
        : (editingDoc.parent_id != null ? resolveDocCategoryId(editingDoc.parent_id) : defaultCategoryId);

    if (selectedCat == null) return dirs;

    return dirs.filter(d => {
      const cat =
        d.category_id != null && d.category_id !== undefined
          ? d.category_id
          : resolveDocCategoryId(d.parent_id);
      return cat === selectedCat || (cat == null && selectedCat === defaultCategoryId);
    });
  }, [editingDoc, flatDocs, isSitePortalPro, defaultCategoryId, categories]);

  const formattedTreeData = renderTreeNodes(displayTreeData);

  const systemClass = 'docs-api-system';

  return (
    <div style={styleSheet.container} className={systemClass}>

      {/* 二级分类：紧挨插件 Tab 下方，不放在左侧大纲内 */}
      {isSitePortalPro && (
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '12px' }}>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: '8px', flex: 1, minWidth: 0 }}>
            {categories.map(c => (
              <button
                key={c.id}
                type="button"
                onClick={() => handleAdminCategoryChange(c.id)}
                style={{
                  border: `1px solid ${activeCategoryId === c.id ? colors.primary : colors.border}`,
                  background: activeCategoryId === c.id ? colors.primary : 'transparent',
                  color: activeCategoryId === c.id ? colors.primaryText : colors.muted,
                  borderRadius: '6px',
                  padding: '4px 12px',
                  fontSize: '13px',
                  cursor: 'pointer',
                  lineHeight: '22px',
                }}
              >
                {c.name}
              </button>
            ))}
          </div>
          <Space size={4}>
            <Popconfirm
              title={t('reset_confirm_title')}
              description={t('reset_confirm_desc')}
              onConfirm={handleResetInitializeDocs}
              okText={t('reset_btn')}
              cancelText={t('cancel_btn')}
              okButtonProps={{ danger: true }}
            >
              <Tooltip title={t('reset_tooltip')}>
                <Button
                  type="text"
                  size="small"
                  danger
                  loading={loading}
                  icon={<ReloadOutlined style={{ fontSize: '14px' }} />}
                  aria-label={t('reset_tooltip')}
                />
              </Tooltip>
            </Popconfirm>
            <Popconfirm
              title={t('clear_confirm_title')}
              description={t('clear_confirm_desc')}
              onConfirm={handleClearAllDocs}
              okText={t('delete_btn')}
              cancelText={t('cancel_btn')}
              okButtonProps={{ danger: true }}
            >
              <Tooltip title={t('clear_tooltip')}>
                <Button
                  type="text"
                  size="small"
                  danger
                  loading={loading}
                  icon={<DeleteOutlined style={{ fontSize: '14px' }} />}
                  aria-label={t('clear_tooltip')}
                />
              </Tooltip>
            </Popconfirm>
            <Tooltip title="管理分类">
              <Button
                type="text"
                size="small"
                icon={<SettingOutlined style={{ fontSize: '14px', color: colors.muted }} />}
                onClick={() => setCategoryModalVisible(true)}
                aria-label="管理分类"
              />
            </Tooltip>
          </Space>
        </div>
      )}

      <div style={styleSheet.body}>
      {/* 左侧侧边栏 - 文档大纲 */}
      <div style={styleSheet.sidebar}>
        {/* 大纲标题 */}
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: '10px' }}>
          <span style={{ fontSize: '14px', fontWeight: 600, letterSpacing: '-0.01em' }}>{t('doc_outline')}</span>
          {!isSitePortalPro && (
            <Space size={4}>
              <Popconfirm
                title={t('reset_confirm_title')}
                description={t('reset_confirm_desc')}
                onConfirm={handleResetInitializeDocs}
                okText={t('reset_btn')}
                cancelText={t('cancel_btn')}
                okButtonProps={{ danger: true }}
              >
                <Tooltip title={t('reset_tooltip')}>
                  <Button
                    type="text"
                    size="small"
                    danger
                    loading={loading}
                    icon={<ReloadOutlined style={{ fontSize: '14px' }} />}
                    aria-label={t('reset_tooltip')}
                  />
                </Tooltip>
              </Popconfirm>
              <Popconfirm
                title={t('clear_confirm_title')}
                description={t('clear_confirm_desc')}
                onConfirm={handleClearAllDocs}
                okText={t('delete_btn')}
                cancelText={t('cancel_btn')}
                okButtonProps={{ danger: true }}
              >
                <Tooltip title={t('clear_tooltip')}>
                  <Button
                    type="text"
                    size="small"
                    danger
                    loading={loading}
                    icon={<DeleteOutlined style={{ fontSize: '14px' }} />}
                    aria-label={t('clear_tooltip')}
                  />
                </Tooltip>
              </Popconfirm>
            </Space>
          )}
        </div>

        {/* 搜索框 */}
        <div style={{ position: 'relative', marginBottom: '10px' }}>
          <SearchOutlined style={{ position: 'absolute', left: '10px', top: '11px', color: colors.muted, zIndex: 2 }} />
          <input
            className="shadcn-input"
            style={{ ...styleSheet.input, paddingLeft: '30px' }}
            placeholder={t('search_placeholder')}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
          />
        </div>

        {/* 树控件 */}
        <div className="docs-sidebar-scroll" style={{ flex: 1, overflowY: 'auto', marginBottom: '14px' }}>
          {formattedTreeData.length === 0 ? (
            <Empty description={t('no_docs')} image={Empty.PRESENTED_IMAGE_SIMPLE} style={{ marginTop: '20px' }} />
          ) : (
            <Tree
              draggable
              blockNode
              selectable
              allowDrop={({ dropNode, dropPosition }) => {
                const target = flatDocs.find(d => d.id === Number(dropNode.key));
                // 仅允许放入目录节点内部
                if (dropPosition === 0 && target && !target.is_dir) return false;
                return true;
              }}
              onSelect={handleSelect}
              selectedKeys={selectedKey ? [String(selectedKey)] : []}
              onDrop={onDrop}
              treeData={formattedTreeData}
              expandedKeys={expandedKeys}
              onExpand={(keys) => setExpandedKeys(keys)}
              style={{ background: 'transparent', color: colors.foreground }}
            />
          )}
        </div>

        {/* 底部新增按钮组 */}
        <div style={{ display: 'flex', gap: '8px', borderTop: `1px solid ${colors.border}`, paddingTop: '12px' }}>
          <button 
            style={{ ...styleSheet.buttonOutline, flex: 1 }} 
            onClick={() => openCreateModal(null, 1)}
          >
            <FolderAddOutlined /> {t('new_dir')}
          </button>
          <button 
            style={{ ...styleSheet.buttonOutline, flex: 1 }} 
            onClick={() => openCreateModal(null, 0)}
          >
            <FileAddOutlined /> {t('new_doc')}
          </button>
        </div>
      </div>

      {/* 右侧主编辑工作区 */}
      <div style={styleSheet.main}>
        {detailLoading && !editingDoc ? (
          <div style={{ display: 'flex', flex: 1, alignItems: 'center', justifyContent: 'center', height: '100%' }}>
            <Spin description="加载文档中..." />
          </div>
        ) : editingDoc ? (
          <div key={`doc-workspace-${editingDoc.id}`} style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
            {/* 工作区 Header */}
            <div style={styleSheet.header}>
              <div style={{ display: 'flex', alignItems: 'center', gap: '8px', minWidth: 0, flex: 1 }}>
                {editingDoc.is_dir ? (
                  <FolderOutlined style={{ color: colors.muted, fontSize: '18px' }} />
                ) : (
                  <FileTextOutlined style={{ color: colors.muted, fontSize: '18px' }} />
                )}
                <input
                  key={`doc-title-${editingDoc.id}-${activeLang}`}
                  value={activeLang === 'zh' ? editingDoc.title : (editingDoc.translations?.[activeLang]?.title || '')}
                  onChange={(e) => {
                    const nextTitle = e.target.value;
                    setEditingDoc(prev => {
                      if (!prev) return prev;
                      if (activeLang === 'zh') {
                        return { ...prev, title: nextTitle };
                      }
                      return {
                        ...prev,
                        translations: {
                          ...prev.translations,
                          [activeLang]: {
                            ...prev.translations?.[activeLang],
                            title: nextTitle,
                            content: prev.translations?.[activeLang]?.content || '',
                          }
                        }
                      };
                    });
                  }}
                  style={{
                    border: 'none',
                    background: 'transparent',
                    fontSize: '16px',
                    fontWeight: 600,
                    color: colors.foreground,
                    outline: 'none',
                    width: '100%',
                    maxWidth: '520px',
                  }}
                  placeholder={t('title_placeholder')}
                />
              </div>
              <button 
                style={styleSheet.buttonPrimary} 
                onClick={handleSave}
              >
                <SaveOutlined /> {t('save_publish')}
              </button>
            </div>

            {/* 精简属性行 */}
            <div style={styleSheet.metaRow}>
              {/* 文档分类（门户增强）：先选分类，再选该分类下的上级目录 */}
              {isSitePortalPro && (
                <div style={styleSheet.metaItem}>
                  <span style={styleSheet.label}>文档分类:</span>
                  <select
                    style={{ ...styleSheet.select, minWidth: '140px' }}
                    value={
                      editingDoc.category_id != null && editingDoc.category_id !== undefined
                        ? editingDoc.category_id
                        : (resolveDocCategoryId(editingDoc.parent_id) ?? '')
                    }
                    onChange={(e) => {
                      const nextCat = e.target.value ? Number(e.target.value) : null;
                      setEditingDoc(prev => {
                        if (!prev) return prev;
                        let nextParent = prev.parent_id;
                        if (nextParent != null) {
                          const parentCat = resolveDocCategoryId(nextParent);
                          if (parentCat !== nextCat) nextParent = null;
                        }
                        return { ...prev, category_id: nextCat, parent_id: nextParent };
                      });
                    }}
                  >
                    <option value="">未分类</option>
                    {categories.map(c => (
                      <option key={c.id} value={c.id}>{c.name}</option>
                    ))}
                  </select>
                </div>
              )}

              {/* 上级目录：门户增强下仅列出当前分类对应的目录 */}
              <div style={styleSheet.metaItem}>
                <span style={styleSheet.label}>{t('parent_dir')}:</span>
                <select
                  style={{ ...styleSheet.select, minWidth: '180px' }}
                  value={editingDoc.parent_id || ''}
                  onChange={(e) => {
                    const val = e.target.value ? Number(e.target.value) : null;
                    setEditingDoc(prev => {
                      if (!prev) return prev;
                      if (!isSitePortalPro || val == null) {
                        return { ...prev, parent_id: val };
                      }
                      const inheritedCat = resolveDocCategoryId(val);
                      return {
                        ...prev,
                        parent_id: val,
                        category_id: inheritedCat ?? prev.category_id ?? null,
                      };
                    });
                  }}
                >
                  <option value="">
                    {isSitePortalPro ? '顶级目录（当前分类）' : t('parent_root')}
                  </option>
                  {parentDirOptions.map(d => (
                    <option key={d.id} value={d.id}>{d.title}</option>
                  ))}
                </select>
              </div>

              {/* 排序权重 */}
              <div style={styleSheet.metaItem}>
                <span style={styleSheet.label}>{t('sort_order')}:</span>
                <InputNumber
                  min={0}
                  size="small"
                  value={editingDoc.sort_order}
                  onChange={(val) => setEditingDoc(prev => prev ? { ...prev, sort_order: val || 0 } : prev)}
                  style={{ width: '70px', borderRadius: '4px', border: `1px solid ${colors.border}`, background: colors.input, color: colors.foreground }}
                />
              </div>

              {/* 路由别名 (slug) */}
              <div style={styleSheet.metaItem}>
                <span style={styleSheet.label}>{t('slug')}:</span>
                <Input
                  size="small"
                  value={editingDoc.slug || ''}
                  onChange={(e) => {
                    const slug = e.target.value;
                    setEditingDoc(prev => prev ? { ...prev, slug } : prev);
                  }}
                  placeholder={t('slug_placeholder')}
                  style={{ width: '130px', borderRadius: '4px', border: `1px solid ${colors.border}`, background: colors.input, color: colors.foreground }}
                />
              </div>

              {/* 启用状态 */}
              <div style={styleSheet.metaItem}>
                <span style={styleSheet.label}>{t('public_visible')}:</span>
                <Switch
                  size="small"
                  checked={editingDoc.is_active === 1}
                  onChange={(checked) => setEditingDoc(prev => prev ? { ...prev, is_active: checked ? 1 : 0 } : prev)}
                />
              </div>
            </div>

            {/* 语言页签选项卡 */}
            <div style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              padding: '8px 20px',
              borderBottom: `1px solid ${colors.border}`,
              background: colors.card,
            }}>
              <div style={{ display: 'flex', gap: '8px' }}>
                {LANGUAGES.map(lang => {
                  const isActive = activeLang === lang.key;
                  return (
                    <button
                      key={lang.key}
                      onClick={() => setActiveLang(lang.key as any)}
                      style={{
                        padding: '6px 12px',
                        borderRadius: '4px',
                        fontSize: '13px',
                        fontWeight: 500,
                        cursor: 'pointer',
                        border: 'none',
                        background: isActive ? colors.accent : 'transparent',
                        color: isActive ? colors.foreground : colors.muted,
                        transition: 'all 0.2s',
                      }}
                    >
                      {lang.label}
                    </button>
                  );
                })}
              </div>

              {activeLang === 'zh' && (
                <button
                  style={{
                    ...styleSheet.buttonOutline,
                    height: '28px',
                    borderColor: '#10b981',
                    color: '#10b981',
                    fontSize: '12px',
                  }}
                  onClick={handleAiTranslateAll}
                  disabled={translating}
                >
                  {t('ai_translate_btn')}
                </button>
              )}
            </div>

            {/* 编辑与预览面板 (集成 md-editor-rt) */}
            {editingDoc.is_dir === 0 ? (
              <div style={styleSheet.editorWrapper}>
                <MdEditor
                  key={`doc-editor-${editingDoc.id}-${activeLang}`}
                  ref={editorRef}
                  modelValue={activeLang === 'zh' ? editingDoc.content : (editingDoc.translations?.[activeLang]?.content || '')}
                  onChange={(val) => {
                    const docId = editingDoc.id;
                    const lang = activeLang;
                    setEditingDoc(prev => {
                      // 切换文档时忽略旧编辑器实例的迟到 onChange，避免标题被上一篇覆盖
                      if (!prev || prev.id !== docId) return prev;
                      if (lang === 'zh') {
                        return { ...prev, content: val };
                      }
                      return {
                        ...prev,
                        translations: {
                          ...prev.translations,
                          [lang]: {
                            ...prev.translations?.[lang],
                            title: prev.translations?.[lang]?.title || '',
                            content: val,
                          }
                        }
                      };
                    });
                  }}
                  theme={themeMode === 'dark' ? 'dark' : 'light'}
                  previewTheme="github"
                  toolbars={[
                    'bold',
                    'underline',
                    'italic',
                    '-',
                    'strikeThrough',
                    'title',
                    'sub',
                    'sup',
                    'quote',
                    'unorderedList',
                    'orderedList',
                    'task',
                    '-',
                    'codeRow',
                    'code',
                    'link',
                    'image',
                    'table',
                    '-',
                    'revoke',
                    'next',
                    '=',
                    'pageFullscreen',
                    'fullscreen',
                    'preview',
                    'previewOnly',
                    'htmlPreview',
                    'catalog'
                  ]}
                  style={{ height: '100%', border: 'none', background: 'transparent' }}
                  placeholder={t('editor_placeholder')}
                />
              </div>
            ) : (
              <div style={{ flex: 1, display: 'flex', flexDirection: 'column', background: colors.background, padding: '40px', overflowY: 'auto' }}>
                <div style={{ maxWidth: '600px', margin: '0 auto', width: '100%' }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: '12px', marginBottom: '24px', borderBottom: `1px solid ${colors.border}`, paddingBottom: '16px' }}>
                    <FolderOutlined style={{ fontSize: '32px', color: colors.muted }} />
                    <div>
                      <h3 style={{ fontSize: '16px', fontWeight: 600, color: colors.foreground, margin: 0 }}>{t('cat_settings')}</h3>
                      <p style={{ fontSize: '13px', color: colors.muted, margin: '4px 0 0 0' }}>{t('cat_settings_desc')}</p>
                    </div>
                  </div>

                  <div style={{ display: 'flex', flexDirection: 'column', gap: '20px' }}>
                    {/* 分类名称编辑 */}
                    <div>
                      <label style={{ ...styleSheet.label, display: 'block', marginBottom: '8px' }}>{t('cat_name')}</label>
                      <Input
                        value={activeLang === 'zh' ? editingDoc.title : (editingDoc.translations?.[activeLang]?.title || '')}
                        onChange={(e) => {
                          if (activeLang === 'zh') {
                            setEditingDoc({ ...editingDoc, title: e.target.value });
                          } else {
                            setEditingDoc({
                              ...editingDoc,
                              translations: {
                                ...editingDoc.translations,
                                [activeLang]: {
                                  ...editingDoc.translations?.[activeLang],
                                  title: e.target.value,
                                  content: editingDoc.translations?.[activeLang]?.content || '',
                                }
                              }
                            });
                          }
                        }}
                        placeholder={t('form_title_dir_placeholder2')}
                        className="shadcn-input"
                        style={{ height: '36px', borderRadius: '6px', border: `1px solid ${colors.border}`, background: colors.input, color: colors.foreground }}
                      />
                    </div>

                    {/* 路由别名编辑 */}
                    <div>
                      <label style={{ ...styleSheet.label, display: 'block', marginBottom: '8px' }}>{t('form_slug_label')}</label>
                      <Input
                        value={editingDoc.slug || ''}
                        onChange={(e) => setEditingDoc({ ...editingDoc, slug: e.target.value })}
                        placeholder={t('slug_dir_placeholder')}
                        className="shadcn-input"
                        style={{ height: '36px', borderRadius: '6px', border: `1px solid ${colors.border}`, background: colors.input, color: colors.foreground }}
                      />
                      <span style={{ fontSize: '12px', color: colors.muted, display: 'block', marginTop: '6px' }}>
                        {t('slug_dir_desc')}
                      </span>
                    </div>

                    <div style={{ display: 'flex', gap: '16px', marginTop: '12px' }}>
                      <div style={{ flex: 1 }}>
                        <label style={{ ...styleSheet.label, display: 'block', marginBottom: '8px' }}>{t('sort_order')}</label>
                        <InputNumber
                          min={0}
                          value={editingDoc.sort_order}
                          onChange={(val) => setEditingDoc({ ...editingDoc, sort_order: val || 0 })}
                          style={{ width: '100%', borderRadius: '6px', border: `1px solid ${colors.border}`, background: colors.input, color: colors.foreground }}
                        />
                      </div>
                      <div style={{ display: 'flex', flexDirection: 'column', justifyContent: 'center', minWidth: '120px' }}>
                        <label style={{ ...styleSheet.label, display: 'block', marginBottom: '8px' }}>{t('public_visible')}</label>
                        <div style={{ display: 'flex', alignItems: 'center', height: '32px' }}>
                          <Switch
                            checked={editingDoc.is_active === 1}
                            onChange={(checked) => setEditingDoc({ ...editingDoc, is_active: checked ? 1 : 0 })}
                          />
                          <span style={{ marginLeft: '8px', fontSize: '13px', color: colors.muted }}>{t('direct_public')}</span>
                        </div>
                      </div>
                    </div>

                    {/* 提示信息 */}
                    <div style={{ marginTop: '24px', padding: '16px', borderRadius: '8px', background: colors.cardMuted, border: `1px solid ${colors.border}` }}>
                      <h4 style={{ fontSize: '13px', fontWeight: 600, color: colors.foreground, margin: '0 0 6px 0' }}>{t('tips_title')}</h4>
                      <p style={{ fontSize: '12px', color: colors.muted, margin: 0, lineHeight: 1.6 }}>
                        {t('tips_desc')}
                      </p>
                    </div>
                  </div>
                </div>
              </div>
            )}
          </div>
        ) : (
          <div style={{ display: 'flex', flex: 1, alignItems: 'center', justifyContent: 'center', height: '100%', padding: '40px' }}>
            <Empty
              description={
                <div style={{ color: colors.muted }}>
                  <h3 style={{ fontSize: '16px', fontWeight: 600, color: colors.foreground, margin: 0 }}>{t('system_title')}</h3>
                  <p style={{ fontSize: '13px', margin: '8px 0 0 0' }}>{t('system_desc')}</p>
                </div>
              }
            />
          </div>
        )}
      </div>
      </div>

      {/* 模态框：新建分类/文档 */}
      <Modal
        title={createIsDir === 1 ? t('create_dir_title') : t('create_doc_title')}
        open={createModalVisible}
        onOk={handleCreateSubmit}
        onCancel={() => setCreateModalVisible(false)}
        okButtonProps={{ style: { background: colors.primary, color: colors.primaryText, border: 'none' } }}
        cancelText={t('cancel')}
        okText={t('create')}
      >
        <Form form={createForm} layout="vertical" style={{ marginTop: '16px' }}>
          <Form.Item
            name="title"
            label={t('form_title')}
            rules={[{ required: true, message: t('form_title_required') }]}
          >
            <Input 
              placeholder={createIsDir === 1 ? t('form_title_dir_placeholder') : t('form_title_doc_placeholder')} 
              className="shadcn-input"
              style={{ height: '36px', borderRadius: '6px' }}
            />
          </Form.Item>
          <Form.Item
            name="slug"
            label={t('form_slug_label')}
            help={createIsDir === 1 ? t('form_slug_help_dir') : t('form_slug_help_doc')}
          >
            <Input 
              placeholder={t('form_slug_placeholder')} 
              className="shadcn-input"
              style={{ height: '36px', borderRadius: '6px' }}
            />
          </Form.Item>
          {isSitePortalPro && (
            <Form.Item
              name="category_id"
              label="文档分类"
              extra={createParentId ? '已选择上级目录时，将自动归属到该目录所在分类' : '选择后，新建节点会出现在对应分类下'}
              getValueFromEvent={(e: React.ChangeEvent<HTMLSelectElement>) => {
                const v = e.target.value;
                return v ? Number(v) : undefined;
              }}
            >
              <select
                style={{ ...styleSheet.select, width: '100%' }}
                disabled={!!createParentId}
              >
                <option value="">未分类</option>
                {categories.map(c => (
                  <option key={c.id} value={c.id}>{c.name}</option>
                ))}
              </select>
            </Form.Item>
          )}
          <Row gutter={16}>
            <Col span={12}>
              <Form.Item
                name="sort_order"
                label={t('form_sort_order_label')}
                initialValue={10}
              >
                <InputNumber min={0} style={{ width: '100%', borderRadius: '6px' }} />
              </Form.Item>
            </Col>
            <Col span={12}>
              <Form.Item
                name="is_active"
                label={t('form_status_label')}
                valuePropName="checked"
                initialValue={true}
              >
                <Switch />
              </Form.Item>
              <span style={{ fontSize: '13px', color: colors.muted, marginTop: '-8px', display: 'block' }}>
                {t('form_status_help')}
              </span>
            </Col>
          </Row>
        </Form>
      </Modal>

      {/* 模态框：管理分类 (Site Portal Pro) */}
      <Modal
        title="管理文档分类"
        open={categoryModalVisible}
        onCancel={() => {
          setCategoryModalVisible(false);
          setEditingCategory(null);
          categoryForm.resetFields();
        }}
        footer={null}
        width={500}
      >
        <div style={{ marginBottom: '16px' }}>
          <Form
            form={categoryForm}
            layout="inline"
            onFinish={async (values) => {
              try {
                const payload = {
                  name: values.name,
                  sort_order: Number(values.sort_order ?? 10),
                };
                if (editingCategory) {
                  await request.put(`${apiPrefix}/docs/categories/${editingCategory.id}`, payload);
                  message.success('更新成功');
                } else {
                  await request.post(`${apiPrefix}/docs/categories`, payload);
                  message.success('创建成功');
                }
                setEditingCategory(null);
                categoryForm.resetFields();
                fetchCategories();
              } catch (e) {
                message.error('操作失败');
              }
            }}
          >
            <Form.Item name="name" rules={[{ required: true, message: '请输入分类名称' }]}>
              <Input placeholder="分类名称，如：API 参考" style={{ width: '180px' }} />
            </Form.Item>
            <Form.Item name="sort_order" initialValue={10}>
              <InputNumber min={0} placeholder="排序(小在前)" style={{ width: '120px' }} />
            </Form.Item>
            <Form.Item>
              <Button type="primary" htmlType="submit">
                {editingCategory ? '保存修改' : '添加新分类'}
              </Button>
              {editingCategory && (
                <Button type="link" onClick={() => { setEditingCategory(null); categoryForm.resetFields(); }}>取消</Button>
              )}
            </Form.Item>
          </Form>
        </div>
        <div style={{ maxHeight: '300px', overflowY: 'auto' }}>
          {categories.length === 0 ? (
            <Empty description="暂无分类" image={Empty.PRESENTED_IMAGE_SIMPLE} />
          ) : (
            <ul style={{ listStyle: 'none', padding: 0, margin: 0 }}>
              {categories.map(c => (
                <li key={c.id} style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', padding: '8px', borderBottom: `1px solid ${colors.border}` }}>
                  <span style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                    {c.name}
                    {c.is_default === 1 && <Tag color="green" style={{ margin: 0 }}>默认</Tag>}
                    <small style={{ color: colors.muted }}>(排序: {c.sort_order})</small>
                  </span>
                  <Space>
                    {c.is_default !== 1 && (
                      <Button type="link" size="small" onClick={async () => {
                        try {
                          await request.post(`${apiPrefix}/docs/categories/${c.id}/set-default`);
                          message.success('已设为默认分类');
                          fetchCategories();
                        } catch {
                          message.error('设置失败');
                        }
                      }}>设为默认</Button>
                    )}
                    <Button type="link" size="small" onClick={() => {
                      setEditingCategory(c);
                      categoryForm.setFieldsValue({ name: c.name, sort_order: c.sort_order });
                    }}>编辑</Button>
                    <Popconfirm title="确定删除该分类吗？其下文档将变为未分类。" onConfirm={async () => {
                      try {
                        await request.delete(`${apiPrefix}/docs/categories/${c.id}`);
                        message.success('删除成功');
                        fetchCategories();
                      } catch (e) {
                        message.error('删除失败');
                      }
                    }}>
                      <Button type="link" size="small" danger>删除</Button>
                    </Popconfirm>
                  </Space>
                </li>
              ))}
            </ul>
          )}
        </div>
      </Modal>
    </div>
  );
};

export default DocsManager;
