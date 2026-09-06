# ============================================================
# DFO 震动插件 - x86 inline hook stub (GNU as, i686, Intel syntax)
# 每个 stub: 保存现场 -> 压入事件类型 -> 取出栈参 a2/a3
#           -> call vib_dispatch_common(this,a2,a3,type)
#           -> 还原现场 -> 跳转各自 trampoline
# 说明: thiscall 调用时 ECX=this, 栈 [esp]=a2,[esp+4]=a3
#       pushfd(1) + pushad(1) 后偏移 36, 故 a2=[esp+36] a3=[esp+40]
# ============================================================

.intel_syntax noprefix
.code32
.text

.extern _vib_dispatch_common
.extern _vib_dispatch_common_hit
.extern _vib_dispatch_shake
.extern _vib_dispatch_skillhit
.extern _vib_dispatch_readshake
.extern _vib_dispatch_targetdie
.extern _vib_dispatch_directshake
.extern _vib_tramp_0
.extern _vib_tramp_1
.extern _vib_tramp_2
.extern _vib_tramp_3
.extern _vib_tramp_4
.extern _vib_tramp_5
.extern _vib_tramp_6
.extern _vib_tramp_7
.extern _vib_tramp_8
.extern _vib_tramp_9
.extern _vib_tramp_10
.extern _vib_tramp_11
.extern _vib_tramp_12

# ---- stub 0: OnAttack ----
.globl _vib_stub_0
_vib_stub_0:
    pushfd
    pushad
    push 0
    mov eax, [esp+44]
    push eax
    mov eax, [esp+48]
    push eax
    push ecx
    call _vib_dispatch_common
    add esp, 16
    popad
    popfd
    mov eax, _vib_tramp_0
    jmp eax

# ---- stub 1: OnDamage ----
.globl _vib_stub_1
_vib_stub_1:
    pushfd
    pushad
    push 1
    mov eax, [esp+44]
    push eax
    mov eax, [esp+48]
    push eax
    push ecx
    call _vib_dispatch_common
    add esp, 16
    popad
    popfd
    mov eax, _vib_tramp_1
    jmp eax

# ---- stub 2: OnStartBattle ----
.globl _vib_stub_2
_vib_stub_2:
    pushfd
    pushad
    push 2
    mov eax, [esp+44]
    push eax
    mov eax, [esp+48]
    push eax
    push ecx
    call _vib_dispatch_common
    add esp, 16
    popad
    popfd
    mov eax, _vib_tramp_2
    jmp eax

# ---- stub 3: OnResetComboDamage ----
.globl _vib_stub_3
_vib_stub_3:
    pushfd
    pushad
    push 3
    mov eax, [esp+44]
    push eax
    mov eax, [esp+48]
    push eax
    push ecx
    call _vib_dispatch_common
    add esp, 16
    popad
    popfd
    mov eax, _vib_tramp_3
    jmp eax

# ---- stub 4: OnActionEnd ----
.globl _vib_stub_4
_vib_stub_4:
    pushfd
    pushad
    push 4
    mov eax, [esp+44]
    push eax
    mov eax, [esp+48]
    push eax
    push ecx
    call _vib_dispatch_common
    add esp, 16
    popad
    popfd
    mov eax, _vib_tramp_4
    jmp eax

# ---- stub 5: OnTargetDie (0x7884C0, 怪物死亡 behavior 回调) ----
# push 2 = VEV_TARGET_DIE (注意: 不能用 hook 索引 5, 那是 VEV_ACTION_END)
.globl _vib_stub_5
_vib_stub_5:
    pushfd
    pushad
    push 2
    mov eax, [esp+44]
    push eax
    mov eax, [esp+48]
    push eax
    push ecx
    call _vib_dispatch_common
    add esp, 16
    popad
    popfd
    mov eax, _vib_tramp_5
    jmp eax

# ---- stub 6: onShakeInput ----
.globl _vib_stub_6
_vib_stub_6:
    pushfd
    pushad
    push 6
    mov eax, [esp+44]
    push eax
    mov eax, [esp+48]
    push eax
    push ecx
    call _vib_dispatch_common
    add esp, 16
    popad
    popfd
    mov eax, _vib_tramp_6
    jmp eax

# ---- stub 7: damage font (sub_E010E0, 每次伤害飘字必经) ----
# 参数: ecx=this 栈 a2..a11; 取 a2/a3/a6(类型标志) -> dispatch_hit(this,a2,a3,a6,type)
.globl _vib_stub_7
_vib_stub_7:
    pushfd
    pushad
    push 1
    mov eax, [esp+60]
    push eax
    mov eax, [esp+52]
    push eax
    mov eax, [esp+52]
    push eax
    push ecx
    call _vib_dispatch_common_hit
    add esp, 20
    popad
    popfd
    mov eax, _vib_tramp_7
    jmp eax

# ---- stub 8: 技能震屏 (sub_1E25540, 广义相机震动汇聚点) ----
# 签名: thiscall(ecx=this, int a2=强度float, int a3=时间, ...)
# 进入时 [esp]=返回地址, [esp+4]=a2(强度), [esp+8]=a3(时间)
# 采集 -> vib_dispatch_skillhit(this, strength, time, retaddr, type=8)
.globl _vib_stub_8
_vib_stub_8:
    pushfd
    pushad
    push 8
    mov eax, [esp+40]
    push eax
    mov eax, [esp+52]
    push eax
    mov eax, [esp+52]
    push eax
    push ecx
    call _vib_dispatch_skillhit
    add esp, 20
    popad
    popfd
    mov eax, _vib_tramp_8
    jmp eax

# ---- stub 9: 真镜头震动 [shake screen] (sub_1E268F0 唯一转发入口) ----
# 签名: thiscall(ecx=this, __int64 a2, ...); [esp+4]=a2低32(强度), [esp+8]=a2高32(时间)
.globl _vib_stub_9
_vib_stub_9:
    pushfd
    pushad
    push 9
    mov eax, [esp+48]
    push eax
    mov eax, [esp+48]
    push eax
    push ecx
    call _vib_dispatch_shake
    add esp, 16
    popad
    popfd
    mov eax, _vib_tramp_9
    jmp eax

# ---- stub 10: 读条震屏 (sub_1E25760, 暴走等 buff 读条结束) ----
# 签名: thiscall(ecx=a1, ebx=a2, [esp+4]=a3(标志float), [esp+8]=a4(强度float))
# 采集 -> vib_dispatch_readshake(this, flag_bits, strength_bits, type=10)
.globl _vib_stub_10
_vib_stub_10:
    pushfd
    pushad
    push 10
    mov eax, [esp+48]
    push eax
    mov eax, [esp+48]
    push eax
    push ecx
    call _vib_dispatch_readshake
    add esp, 16
    popad
    popfd
    mov eax, _vib_tramp_10
    jmp eax

# ---- stub 11: 怪物死亡 (sub_F20BF0, 被击者 HP<=0 确认死亡后调用) ----
# 签名: thiscall(ecx=攻击者, [esp+4]=被击者, [esp+8]=a3)
# 采集 -> vib_dispatch_targetdie(victim=[esp+4], type=11)
.globl _vib_stub_11
_vib_stub_11:
    pushfd
    pushad
    push 11
    mov eax, [esp+44]
    push eax
    call _vib_dispatch_targetdie
    add esp, 8
    popad
    popfd
    mov eax, _vib_tramp_11
    jmp eax

# ---- stub 12: 直接震屏 (sub_1E26F00, 直接写相机震屏字段) ----
# 签名: thiscall(ecx=相机, [esp+4]=a2(强度系数float), ...)
# 采集 -> vib_dispatch_directshake(this, strength_bits, type=12)
.globl _vib_stub_12
_vib_stub_12:
    pushfd
    pushad
    push 12
    mov eax, [esp+44]
    push eax
    push ecx
    call _vib_dispatch_directshake
    add esp, 12
    popad
    popfd
    mov eax, _vib_tramp_12
    jmp eax

.end
