import type { AppSettings } from '../../shared/types'
import { FIXED_NODE_HEALTH_NOTE, HEALTH_SETTING_KEYS } from './Settings'

const healthMonitorKey: 'healthMonitorEnabled' = HEALTH_SETTING_KEYS.healthMonitorEnabled
const mainAutoFailoverKey: 'mainNodeAutoFailover' = HEALTH_SETTING_KEYS.mainNodeAutoFailover

const healthSettingsPatch: Pick<AppSettings, 'healthMonitorEnabled' | 'mainNodeAutoFailover'> = {
  [healthMonitorKey]: true,
  [mainAutoFailoverKey]: false,
}

const fixedNodeNote: '固定节点分流失败时只提示，不会自动更换。' = FIXED_NODE_HEALTH_NOTE

void healthSettingsPatch
void fixedNodeNote
