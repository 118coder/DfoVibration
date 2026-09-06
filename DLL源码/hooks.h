#ifndef VIB_HOOKS_H
#define VIB_HOOKS_H

#include <windows.h>

#define VIB_MAX_HOOKS 16

/* 安装/卸载 7 个事件 hook。
 * targets: 目标函数地址数组(与 g_hookTargets 一致)
 * count:   hook 数量
 * 返回: 成功安装的数量 */
int  hooks_install(const DWORD *targets, int count);

/* 全部卸载(还原原始字节) */
void hooks_uninstall(void);

/* 是否已安装 */
int  hooks_active(void);

/* 采集分发(汇编 stub 调用, 游戏线程上下文, 必须极短) */
void __cdecl vib_dispatch_common(void *obj, DWORD a2, DWORD a3, int type);

/* 镜头震动采集: obj=相机管理器, strength_bits=强度(float 位模式), time_ms=持续时间 */
void __cdecl vib_dispatch_shake(void *obj, DWORD strength_bits, DWORD time_ms, int type);

/* 技能震动采集 (广义相机震动): hook sub_1E25540
 * retaddr = 调用返回地址, 用于区分技能震动 vs 暴击/击杀特写 */
void __cdecl vib_dispatch_skillhit(void *obj, DWORD strength_bits, DWORD time_ms, DWORD retaddr, int type);

/* 读条震屏采集 (暴走等 buff 读条结束): hook sub_1E25760
 * flag_bits = a3(float 位模式, 低字节=震屏标志 0/1), strength_bits = a4(float 强度) */
void __cdecl vib_dispatch_readshake(void *obj, DWORD flag_bits, DWORD strength_bits, int type);

/* 怪物死亡采集: hook sub_F20BF0 (仅在被击者 HP<=0 确认死亡后由 sub_1C58AC0 调用)
 * obj = 被击者 (栈参数 [esp+4]), 排除玩家自身 (n2500) */
void __cdecl vib_dispatch_targetdie(void *obj, int type);

/* 直接震屏采集: hook sub_1E26F00 (直接写相机震屏字段, 不经过 sub_1E25540/25760)
 * 技能/场景在"目标容器为空"时调用 (普通技能不命中也震), 时间固定 200ms
 * strength_bits = a2(float 强度系数, 调用点 100.0) */
void __cdecl vib_dispatch_directshake(void *obj, DWORD strength_bits, int type);

/* 评分等级写入分发: obj=评分对象, level=评分等级(2~8) */
void __cdecl vib_dispatch_rank(void *obj, DWORD level, int type);

/* 评分采集: 内存轮询 (零 hook, 零 UI 影响), 聚合线程调用 */
void vib_flush_rank(void);
void vib_flush_rank_extra(void);

/* 相机震屏轮询: 相机对象+1952 上升沿 → VEV_SKILL_HIT (覆盖所有震屏路径) */
void poll_cam_shake(void);

/* 背击/破招定位诊断: 解密评分文字 key 标定槽名 (每 60s 一次) */
void diag_rank_texts(void);

/* 评分事件计数器轮询 (背击/破招定位): score+292*槽+136+8*etype 解密读增量 */
void poll_rank_events(void);

/* 破招/背击文字扫描 (只读内存): 找 dstr 解密后 UTF-16 字符串地址 */
void scan_dstr_text(void);

/* 聚合计数冲刷: 由独立聚合线程调用, 批量发送计数事件 */
void vib_flush_counts(void);

/* 供汇编引用的每-hook trampoline 地址 */
extern void *vib_tramp_0;
extern void *vib_tramp_1;
extern void *vib_tramp_2;
extern void *vib_tramp_3;
extern void *vib_tramp_4;
extern void *vib_tramp_5;
extern void *vib_tramp_6;
extern void *vib_tramp_7;
extern void *vib_tramp_8;
extern void *vib_tramp_9;
extern void *vib_tramp_10;
extern void *vib_tramp_11;
extern void *vib_tramp_12;

#endif /* VIB_HOOKS_H */
