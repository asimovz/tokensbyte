/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia 
 * @license        MIT (https://www.tokensbyte.ai/)
 */

import React from 'react';
import { Alert, Typography } from 'antd';

const { Paragraph, Text } = Typography;

const BODY = '您的额度已低于设定提醒额度';

const TIPS: Record<'tencent' | 'volcengine', string> = {
  tencent:
    '腾讯云申请：类型选「通知」；正文勿写签名；无需模板变量。审核通过后填入「余额提醒模板 ID」。',
  volcengine:
    '火山引擎申请：正文勿写签名；无需模板变量。审核通过后填入「余额提醒模板 ID」。',
};

type Props = {
  provider?: 'tencent' | 'volcengine';
  title?: string;
  style?: React.CSSProperties;
};

/** 「消息通知 → 短信」余额模板推荐正文 */
const LowBalanceSmsHint: React.FC<Props> = ({
  provider = 'tencent',
  title,
  style,
}) => {
  const vendor = provider === 'volcengine' ? '火山引擎' : '腾讯云';
  return (
    <Alert
      type="info"
      showIcon
      style={style}
      title={title || `${vendor}余额提醒短信模板（可复制申请）`}
      description={
        <div>
          <Paragraph
            copyable={{ text: BODY, tooltips: ['复制', '已复制'] }}
            style={{
              marginBottom: 6,
              fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace',
              fontSize: 13,
            }}
          >
            {BODY}
          </Paragraph>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {TIPS[provider]}
          </Text>
        </div>
      }
    />
  );
};

export default LowBalanceSmsHint;
