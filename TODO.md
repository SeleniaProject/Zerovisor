### フェーズ1: 最小ブートと能力検出
- [x] タスク: Rust UEFIエントリ（no_std, `x86_64-unknown-uefi`）の雛形を作成
  - 成果物: `Cargo.toml`、`src/efi_main.rs`、`BUILD.md`
  - 目的: UEFI上で起動し、画面に初期化ログを表示（英語メッセージで最小確認）
  - 備考: `.cargo/config.toml` は未作成（ターゲット指定はビルドコマンドで実施）
  - 工数: 中
- [x] タスク: CPUID/MSRユーティリティ（Rust `asm!`）実装
  - 成果物: `src/arch/x86/cpuid.rs`, `src/arch/x86/msr.rs`
  - 目的: VMX/SVM、EPT/NPT、VT‑d/AMD‑VIの存在検出（MSR書込みは未使用／安全側）
  - 工数: 中
- [x] タスク: 多言語ログ（日本語/英語/中国語）
  - 成果物: `src/i18n/mod.rs`, `lang/*.json`
  - 目的: ログメッセージを言語切替（現状は英語既定、将来`PlatformLang`対応予定）
  - 工数: 小

### フェーズ2: ACPIとSMP初期化
- [x] タスク: ACPIテーブル走査（RSDP→XSDT→FADT/MADT/MCFG/HPET）
  - 成果物: `src/firmware/acpi/*.rs`, `src/time/hpet.rs`
  - 目的: CPUトポロジ、APIC情報、PCIe設定空間、HPETの基礎入手
  - 追記: DMAR/IVRSの検出とヘッダ要約・エントリ概要（DRHD/RMRR/ATSR/IVRS entries）を起動時に出力
  - 追記: MCFGからPCIe ECAMセグメント一覧を出力
  - 工数: 中
- [x] タスク: AP起動（SMP bring‑up）とタイムソース初期化
  - 成果物: `src/arch/x86/smp.rs`, `src/time/*.rs`
  - 目的: MADT列挙、INIT+SIPI送出、TSC校正（HPET優先/UEFI Stallフォールバック）
  - 工数: 中
- [x] タスク: リアルモードトランポリン構築とAP同期
  - 成果物: `src/arch/x86/trampoline.rs`
  - 目的: PM/LM到達フラグ、AP ID収集、RSP配列、GO/READY同期、観測カウンタ
  - 工数: 大
- [x] タスク: LAPIC/x2APIC初期化ユーティリティ
  - 成果物: `src/arch/x86/lapic.rs`
  - 目的: APIC ID読取、SVR設定、INIT/SIPI送出、自動x2APIC経路
  - 工数: 中
- [x] タスク: 最小IDTの構築と割り込み有効化
  - 成果物: `src/arch/x86/idt.rs`
  - 目的: 例外発生時の安全停止（トリプルフォールト回避）、STI有効化
  - 工数: 小

### フェーズ3: VMX/SVM有効化と二段ページング
- [x] タスク: VMX/SVMプレフライトと初期化抽象（VMCS/VMCB領域管理含む）
  - 成果物: `src/arch/x86/vm/vmx.rs`, `src/arch/x86/vm/vmcs.rs`, `src/arch/x86/vm/svm.rs`
  - 目的: 可用性検証、CR0/CR4固定ビット反映、IA32_FEATURE_CONTROL検査、VMXON/VMXOFFおよびVMPTRLD/VMCLEARのスモークテスト
  - 追記: VMX制御MSR/EPT_VPID_CAPの報告
  - 工数: 大
- [x] タスク: VMX EPTP設定スモークテスト（起動前検証）
  - 成果物: `src/arch/x86/vm/vmx.rs`, `src/mm/ept.rs`
  - 目的: 恒等マップEPTを生成し、VMCSへEPTP設定まで確認（VMLAUNCHは未実施）
  - 工数: 中
- [x] タスク: EPT/NPTテーブル生成（現状範囲）
  - 成果物: `src/mm/ept.rs`, `src/mm/npt.rs`
  - 目的: 二段変換の恒等マップ生成とEPTR/NCr3構成
  - 現状: EPT=4K/2M/1G対応、NPT=4K/2M/1G対応
  - 未了: A/Dビット運用、詳細属性
  - 工数: 大

### フェーズ4: デバイス仮想化の基礎
- [x] タスク: VirtIOコンソール/ブロック/ネット（最小）
  - 成果物: `src/virtio/mod.rs`, `src/virtio/console.rs`, `src/virtio/block.rs`, `src/virtio/net.rs`
  - 現状: ECAM走査・PCI Vendor Cap走査、virtio-consoleの最小ステータスハンドシェイク（ACK/DRIVER/DRIVER_OK）、virtio‑blk容量レポート、virtio‑net検出レポート
  - 工数: 大
- [x] タスク: VT‑d/AMD‑VI（IOMMU）初期化、デバイス保護ドメイン
  - 成果物: `src/iommu/vtd.rs`, `src/iommu/amdv.rs`
  - 現状/進捗:
    - VT‑d: DRHD列挙、RTADDR設定＋SRTP確認、TEのenable/disable観測、Root/Contextテーブル生成、Context Entry設定（AW/TT/DID/SLPTPTR）、二次PT（4KiB/2MiB）マッピング、SRTP粒度invalidate（all/dom/bdf/hard）、検証/同期/翻訳/歩査/統計CLIを実装
    - AMD‑Vi: IVRS検出・要約・エントリ列挙まで（最小初期化は未実装）
  - 残: CCMD/IOTLBによる正式invalidate（CIRG/IIRG/ICC/IVT、DMAドレイン）実装、AMD‑Vi最小初期化（IVHD→MMIO→Device Table→有効化）
  - 工数: 大
  - 備考: CLIによる管理プレーンからの観測・適用を重視（後述）

### フェーズ5: 管理プレーン最小機能
- [x] タスク: シリアル/UEFIコンソール経由CLI（最小）
  - 成果物: `src/ctl/cli.rs`
  - 現状: `help|info|virtio|iommu|pci|vm|trace|metrics|quit` をサポート
  - 工数: 中
  - 進捗（主要サブコマンド）:
    - `iommu`: `info|units|root <bus>|lsctx <bus>|dump <bus:dev.func>|plan|plan dom=<id>|validate|verify|verify-map|xlate bdf=<seg:bus:dev.func> iova=<hex>|walk bdf=<seg:bus:dev.func> iova=<hex>|apply|apply-refresh|apply-safe|sync|invalidate|invalidate dom=<id>|invalidate bdf=<seg:bus:dev.func>|hard-invalidate|fsts|fclear|stats|summary`
    - `dom`: `new|destroy <id>|purge <id>|seg:bus:dev.func assign <id>|seg:bus:dev.func unassign|list|map dom=<id> iova=<hex> pa=<hex> len=<hex> perm=[rwx]|unmap dom=<id> iova=<hex> len=<hex>|mappings|dump`
    - `vm`: `new|start`
- [x] タスク: VM作成/起動/停止/削除の基本API
  - 成果物: `src/hv/vm.rs`, `src/hv/vcpu.rs`
  - 現状: VM生成ID付与、vCPU開始/停止、Intel=VMXスモーク+EPT接続、AMD=SVMプレフライト+NPT準備（VMLAUNCH相当は未実装）
  - 工数: 大

### フェーズ6: セキュリティと可用性強化
- [ ] タスク: 監査ログ、クラッシュダンプ、ウォッチドッグ
  - 成果物: `src/obs/*.rs`, `src/diag/*.rs`
  - 工数: 中
- [ ] タスク: ライブマイグレーション基盤（前提同期/ダーティページ追跡）
  - 成果物: `src/migrate/*.rs`
  - 工数: 大
 
### 受入基準の詳細（各フェーズ共通）
- [ ] ドキュメント: 設計/要件/仕様更新が反映され、i18n辞書（日/英/中）が同期していること。
- [ ] テレメトリ: ログ/メトリクス/トレースの最小計測点が導入され、受入時に確認可能であること。
- [ ] セキュリティ: W^X/SMEP/SMAPが有効、IOMMU無しのパススルーが禁止されていることを検証。

### 追加タスク（Observability & i18n）
- [x] タスク: 構造化ログ（レベル/カテゴリ/言語タグ）
  - 成果物: `src/obs/log.rs`（最小：UEFIコンソール出力、レベル/カテゴリ付与）
  - 工数: 中
- [x] タスク: メトリクス（カウンタ/ヒストグラム）
  - 成果物: `src/obs/metrics.rs`（最小：VM作成/開始カウンタ）
  - 工数: 中
- [x] タスク: トレース（VM‑Entry/Exit、EPT操作）
  - 成果物: `src/obs/trace.rs`（最小：リングバッファ、CLI `trace`でダンプ）
  - 工数: 中
- [x] タスク: 多言語辞書（CLI/ログ）
  - 成果物: `lang/ja.json`, `lang/en.json`, `lang/zh.json`
  - 工数: 小
- [x] タスク: UEFI `PlatformLang` による動的言語選択
  - 成果物: `src/i18n/mod.rs`
  - 実装: UEFI変数`PlatformLang`（EFI_GLOBAL_VARIABLE）を`RuntimeServices::get_variable`で取得し、`en`/`ja`/`zh`を動的選択（ASCII/RFC3066前方一致、静的バッファ、英語フォールバック）
  - 工数: 小

### パフォーマンス検証タスク（性能上限の可視化）
- [ ] タスク: VM‑Entry/Exit サイクル計測の設計
  - 成果物: 計測ポイント定義、可視化手順
  - 工数: 中
- [ ] タスク: EPT/NPTヒット率・TLBシュートダウン頻度の計測設計
  - 成果物: メトリクス設計、レポート雛形
  - 工数: 中
- [ ] タスク: I/Oゼロコピー率とスループット/レイテンシの計測計画
  - 成果物: 負荷ツール選定と手順書
  - 工数: 中

### リスク対応タスク（Risks & Mitigations）
- [ ] タスク: タイマフォールバック戦略（Invariant TSC→HPET）
  - 成果物: 設計メモ、検証手順
  - 現状: 実装はHPET優先/UEFI Stallフォールバックで動作、文書化と検証残
  - 工数: 小
- [ ] タスク: SR‑IOV/ACS未対応デバイス検出と制限
  - 成果物: 要件とテスト項目
  - 工数: 小
