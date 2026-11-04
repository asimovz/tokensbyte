/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

import React, { useState } from 'react';
import {
  Form, Input, Radio, Select, Upload, Button, Space, Typography, Tag, DatePicker, Image, message, Alert,
} from 'antd';
import { UploadOutlined, DeleteOutlined } from '@ant-design/icons';
import dayjs, { type Dayjs } from 'dayjs';
import request from '../utils/request';
import type { UserKyc, UserKycStatus, UserKycType, KycIdDocType, KycValidityType } from '../types';

const { Text } = Typography;

export const KYC_STATUS_META: Record<UserKycStatus, { label: string; color: string }> = {
  none: { label: '未认证', color: 'default' },
  pending: { label: '审核中', color: 'processing' },
  approved: { label: '已通过', color: 'success' },
  rejected: { label: '已驳回', color: 'error' },
  expired: { label: '已过期', color: 'warning' },
};

const ID_DOC_OPTIONS: { value: KycIdDocType; label: string }[] = [
  { value: 'id_card', label: '身份证' },
  { value: 'passport', label: '护照' },
  { value: 'driver_license', label: '驾照' },
];

export type KycFormValues = {
  kyc_type: UserKycType;
  status?: UserKycStatus;
  real_name?: string;
  id_doc_type?: KycIdDocType;
  id_doc_front_url?: string;
  id_doc_back_url?: string;
  company_name?: string;
  business_license_url?: string;
  tax_registration_url?: string;
  legal_notarization_url?: string;
  validity_type: KycValidityType;
  expire_at?: Dayjs | null;
  reject_reason?: string;
  admin_remark?: string;
};

export function kycToFormValues(kyc?: UserKyc | null): KycFormValues {
  return {
    kyc_type: (kyc?.kyc_type as UserKycType) || 'personal',
    status: (kyc?.status as UserKycStatus) || 'none',
    real_name: kyc?.real_name || undefined,
    id_doc_type: (kyc?.id_doc_type as KycIdDocType) || undefined,
    id_doc_front_url: kyc?.id_doc_front_url || undefined,
    id_doc_back_url: kyc?.id_doc_back_url || undefined,
    company_name: kyc?.company_name || undefined,
    business_license_url: kyc?.business_license_url || undefined,
    tax_registration_url: kyc?.tax_registration_url || undefined,
    legal_notarization_url: kyc?.legal_notarization_url || undefined,
    validity_type: (kyc?.validity_type as KycValidityType) || 'long_term',
    expire_at: kyc?.expire_at ? dayjs(kyc.expire_at) : null,
    reject_reason: kyc?.reject_reason || undefined,
    admin_remark: kyc?.admin_remark || undefined,
  };
}

export function formValuesToKycPayload(values: KycFormValues, includeStatus: boolean) {
  const payload: Record<string, unknown> = {
    kyc_type: values.kyc_type,
    real_name: values.real_name || '',
    id_doc_type: values.id_doc_type || '',
    id_doc_front_url: values.id_doc_front_url || '',
    id_doc_back_url: values.id_doc_back_url || '',
    company_name: values.company_name || '',
    business_license_url: values.business_license_url || '',
    tax_registration_url: values.tax_registration_url || '',
    legal_notarization_url: values.legal_notarization_url || '',
    validity_type: values.validity_type || 'long_term',
    expire_at: values.validity_type === 'expire_date' && values.expire_at
      ? values.expire_at.endOf('day').toISOString()
      : '',
    reject_reason: values.reject_reason || '',
    admin_remark: values.admin_remark || '',
  };
  if (includeStatus && values.status) {
    payload.status = values.status;
  }
  return payload;
}

async function uploadKycFile(file: File, docField: string, targetUserId?: string): Promise<string> {
  const formData = new FormData();
  formData.append('file', file);
  formData.append('doc_field', docField);
  if (targetUserId) formData.append('target_user_id', targetUserId);
  const res = await (request.post('/user/kyc/upload', formData, {
    headers: { 'Content-Type': 'multipart/form-data' },
  }) as unknown as Promise<{ file_url: string }>);
  if (!res?.file_url) throw new Error('上传失败');
  return res.file_url;
}

type DocUploadProps = {
  label: string;
  value?: string;
  onChange?: (url?: string) => void;
  docField: string;
  targetUserId?: string;
  disabled?: boolean;
};

const DocUploadField: React.FC<DocUploadProps> = ({
  label, value, onChange, docField, targetUserId, disabled,
}) => {
  const [uploading, setUploading] = useState(false);
  const isPdf = !!value && /\.pdf($|\?)/i.test(value);

  return (
    <div style={{ marginBottom: 16 }}>
      <div style={{ marginBottom: 8 }}>
        <Text>{label}</Text>
        <Text type="danger"> *</Text>
      </div>
      <Space direction="vertical" style={{ width: '100%' }}>
        {value ? (
          <div style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
            {isPdf ? (
              <a href={value} target="_blank" rel="noreferrer">查看 PDF</a>
            ) : (
              <Image src={value} width={120} style={{ borderRadius: 6, objectFit: 'cover' }} />
            )}
            {!disabled && (
              <Button
                danger
                type="text"
                icon={<DeleteOutlined />}
                onClick={() => onChange?.(undefined)}
              >
                移除
              </Button>
            )}
          </div>
        ) : null}
        {!disabled && (
          <Upload
            showUploadList={false}
            accept="image/*,.pdf,application/pdf"
            beforeUpload={async (file) => {
              if (file.size > 12 * 1024 * 1024) {
                message.error('文件不能超过 12MB');
                return Upload.LIST_IGNORE;
              }
              try {
                setUploading(true);
                const url = await uploadKycFile(file as File, docField, targetUserId);
                onChange?.(url);
                message.success('上传成功');
              } catch (e: any) {
                message.error(e?.message || '上传失败');
              } finally {
                setUploading(false);
              }
              return Upload.LIST_IGNORE;
            }}
          >
            <Button icon={<UploadOutlined />} loading={uploading}>
              {value ? '重新上传' : '上传文件'}
            </Button>
          </Upload>
        )}
      </Space>
    </div>
  );
};

type Props = {
  form: any;
  mode: 'admin' | 'user';
  targetUserId?: string;
  /** 用户端已通过时只读 */
  readOnly?: boolean;
  currentStatus?: UserKycStatus;
};

/** 实名表单字段区（挂到外部 Form 上） */
const UserKycFormFields: React.FC<Props> = ({
  form, mode, targetUserId, readOnly, currentStatus,
}) => {
  const kycType = Form.useWatch('kyc_type', form) as UserKycType | undefined;
  const idDocType = Form.useWatch('id_doc_type', form) as KycIdDocType | undefined;
  const validityType = Form.useWatch('validity_type', form) as KycValidityType | undefined;
  const adminStatus = Form.useWatch('status', form) as UserKycStatus | undefined;
  const statusMeta = KYC_STATUS_META[currentStatus || 'none'];
  const requireDocs = mode === 'user' || adminStatus === 'approved' || adminStatus === 'pending' || !adminStatus;

  return (
    <div style={{ marginTop: 8 }}>
      {currentStatus && currentStatus !== 'none' && (
        <div style={{ marginBottom: 16 }}>
          <Text type="secondary" style={{ marginRight: 8 }}>当前状态</Text>
          <Tag color={statusMeta.color}>{statusMeta.label}</Tag>
        </div>
      )}
      {mode === 'user' && currentStatus === 'rejected' && (
        <Alert
          type="error"
          showIcon
          style={{ marginBottom: 16 }}
          title="实名认证未通过"
          description={form.getFieldValue('reject_reason') || '请根据驳回原因修改后重新提交'}
        />
      )}
      {mode === 'user' && currentStatus === 'pending' && (
        <Alert
          type="info"
          showIcon
          style={{ marginBottom: 16 }}
          title="实名材料审核中，请耐心等待。审核通过前可修改后重新提交。"
        />
      )}

      <Form.Item
        name="kyc_type"
        label="实名类型"
        rules={[{ required: true, message: '请选择实名类型' }]}
        initialValue="personal"
      >
        <Radio.Group disabled={readOnly} optionType="button" buttonStyle="solid">
          <Radio.Button value="personal">个人实名</Radio.Button>
          <Radio.Button value="enterprise">企业实名</Radio.Button>
        </Radio.Group>
      </Form.Item>

      {mode === 'admin' && (
        <Form.Item name="status" label="认证状态" initialValue="approved">
          <Select
            options={[
              { value: 'none', label: '未认证' },
              { value: 'pending', label: '审核中' },
              { value: 'approved', label: '已通过' },
              { value: 'rejected', label: '已驳回' },
              { value: 'expired', label: '已过期' },
            ]}
          />
        </Form.Item>
      )}

      {kycType === 'enterprise' ? (
        <>
          <Form.Item
            name="company_name"
            label="企业名称"
            rules={requireDocs ? [{ required: true, message: '请填写企业名称' }] : []}
          >
            <Input disabled={readOnly} placeholder="营业执照上的企业全称" maxLength={120} />
          </Form.Item>
          <Form.Item
            name="business_license_url"
            rules={requireDocs ? [{ required: true, message: '请上传营业执照' }] : []}
            style={{ marginBottom: 0 }}
          >
            <DocUploadField
              label="营业执照"
              docField="business_license"
              targetUserId={targetUserId}
              disabled={readOnly}
            />
          </Form.Item>
          <Form.Item
            name="tax_registration_url"
            rules={requireDocs ? [{ required: true, message: '请上传税务登记证' }] : []}
            style={{ marginBottom: 0 }}
          >
            <DocUploadField
              label="税务登记证"
              docField="tax_registration"
              targetUserId={targetUserId}
              disabled={readOnly}
            />
          </Form.Item>
          <Form.Item
            name="legal_notarization_url"
            rules={requireDocs ? [{ required: true, message: '请上传企业法务公证' }] : []}
            style={{ marginBottom: 0 }}
          >
            <DocUploadField
              label="企业法务公证"
              docField="legal_notarization"
              targetUserId={targetUserId}
              disabled={readOnly}
            />
          </Form.Item>
        </>
      ) : (
        <>
          <Form.Item
            name="real_name"
            label="真实姓名"
            rules={requireDocs ? [{ required: true, message: '请填写真实姓名' }] : []}
          >
            <Input disabled={readOnly} placeholder="与证件一致的姓名" maxLength={64} />
          </Form.Item>
          <Form.Item
            name="id_doc_type"
            label="证件类型"
            rules={requireDocs ? [{ required: true, message: '请选择证件类型' }] : []}
          >
            <Select disabled={readOnly} placeholder="身份证 / 护照 / 驾照" options={ID_DOC_OPTIONS} />
          </Form.Item>
          <Form.Item
            name="id_doc_front_url"
            rules={requireDocs ? [{ required: true, message: '请上传证件正面/主页' }] : []}
            style={{ marginBottom: 0 }}
          >
            <DocUploadField
              label={idDocType === 'passport' ? '护照个人信息页' : idDocType === 'driver_license' ? '驾照正面' : '身份证正面'}
              docField="id_doc_front"
              targetUserId={targetUserId}
              disabled={readOnly}
            />
          </Form.Item>
          {(idDocType === 'id_card' || !idDocType) && (
            <Form.Item
              name="id_doc_back_url"
              rules={requireDocs && idDocType === 'id_card' ? [{ required: true, message: '请上传身份证反面' }] : []}
              style={{ marginBottom: 0 }}
            >
              <DocUploadField
                label="身份证反面"
                docField="id_doc_back"
                targetUserId={targetUserId}
                disabled={readOnly}
              />
            </Form.Item>
          )}
        </>
      )}

      <Form.Item
        name="validity_type"
        label="有效期"
        initialValue="long_term"
        rules={[{ required: true }]}
      >
        <Radio.Group disabled={readOnly}>
          <Radio value="long_term">长期有效</Radio>
          <Radio value="expire_date">按证件有效期</Radio>
        </Radio.Group>
      </Form.Item>
      {validityType === 'expire_date' && (
        <Form.Item
          name="expire_at"
          label="证件到期日"
          rules={[{ required: true, message: '请选择证件到期日' }]}
        >
          <DatePicker disabled={readOnly} style={{ width: '100%' }} />
        </Form.Item>
      )}

      {mode === 'admin' && (
        <>
          <Form.Item name="reject_reason" label="驳回原因">
            <Input.TextArea rows={2} placeholder="驳回时填写，用户可见" maxLength={500} />
          </Form.Item>
          <Form.Item name="admin_remark" label="管理员备注">
            <Input.TextArea rows={2} placeholder="仅管理员可见" maxLength={500} />
          </Form.Item>
        </>
      )}
    </div>
  );
};

export default UserKycFormFields;
