# 归档：未来存档／正文编辑接线草案

未实现，不是当前模块接口。当前归属见[Flow技术文档](../../backend/flow/technical.md)。

## 存档接线

ChatArchivePort的定义不在本页复制。未来Main只初始化一个存档所有者，Flow持其写端口引用、消息持其查询端口引用，两者指向同一实例；彼此不调用对方实现。Flow的回放是易失内存，不是第二数据库；普通操作日志不复制聊天历史。

只把真实终态／协议事实及缺口异步交存档；不每token刷盘。消息线程调度写任务，不等待提交后才推前端。写入以记录ID和消息revision幂等，旧修订不能覆盖新修订；没有真实提交不发布ArchiveCommit。当前archive=None，历史查询明确NotConfigured，不返回空成功。

## 修改内部接口预留

```text
ChatEditPlan {
  exchange: ExchangeKey, target: ChatEditTarget,
  expected_revision: u64, expected_private_digest: [u8;32],
  profile: ProviderProfile, replacement: PrivateProviderContent
}
TlsMediator::edit(plan: ChatEditPlan, unit: OwnedUnsentUnit)
    -> async Result<ChatEditReceipt, FlowError>
```

ChatEditPlan、PrivateProviderContent、OwnedUnsentUnit均为Flow私有类型，不出现在中间层命令、配置源或驱动帧。OwnedUnsentUnit由真实传输所有者创建，不能从CaptureChunk／复制FD／用户指针构造；通用正文修改本轮不启用。幻影凭证使用已明确的专属认证出口，不通过聊天正文edit或前端脱敏副本绕行。

允许修改的协议单元及一致性只依赖编解码文档。已开始发送后由原任务保管，取消／完成竞争同一次终态；Partial不能重放请求。中介连接关闭不能无缝回退原TLS。停止Flow仅释放自身资源，不终止目标或外部Root broker。
