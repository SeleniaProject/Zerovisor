### フェーズ1: 最小ブートと能力検出
- [x] タスク: Rust UEFIエントリ（no_std, `x86_64-unknown-uefi`）の雛形を作成
  - 成果物: `Cargo.toml`、`src/efi_main.rs`、`BUILD.md`
  - 目的: UEFI上で起動し、画面に初期化ログを表示（英語メッセージで最小確認）
  - 備考: `.cargo/config.toml` は未作成（ターゲット指定はビルドコマンドで実施）
  - 工数: 中
- [ ] タスク: CPUID/MSRユーティリティ（Rust `asm!`）実装
  - 成果物: `src/arch/x86/cpuid.rs`, `src/arch/x86/msr.rs`
  - 目的: VMX/SVM、EPT/NPT、VT‑d/AMD‑VIの存在検出
  - 工数: 中
- [ ] タスク: 多言語ログ（日本語/英語最小）
  - 成果物: `src/i18n/mod.rs`, `lang/*.json`
  - 目的: ログメッセージを言語切替
  - 工数: 小

### フェーズ2: ACPIとSMP初期化
- [ ] タスク: ACPIテーブル走査（RSDP→XSDT→FADT/MADT/MCFG）
  - 成果物: `src/firmware/acpi/*.rs`
  - 目的: CPUトポロジ、APIC情報、PCIe設定空間の基礎入手
  - 工数: 中
- [ ] タスク: AP起動（SMP bring‑up）とタイムソース初期化
  - 成果物: `src/arch/x86/smp.rs`, `src/time/*.rs`
  - 工数: 中

### フェーズ3: VMX/SVM有効化と二段ページング
- [ ] タスク: VMXON/VMCS（Intel）とVMCB（AMD）初期化抽象
  - 成果物: `src/arch/x86/vm/vmx.rs`, `src/arch/x86/vm/svm.rs`, `src/arch/x86/vm/mod.rs`
  - 工数: 大
- [ ] タスク: EPT/NPTテーブル生成（4K/2M/1G対応）
  - 成果物: `src/mm/ept.rs`, `src/mm/npt.rs`
  - 工数: 大

### フェーズ4: デバイス仮想化の基礎
- [ ] タスク: VirtIOコンソール/ブロック/ネット（最小）
  - 成果物: `src/virtio/*.rs`
  - 工数: 大
- [ ] タスク: VT‑d/AMD‑VI（IOMMU）初期化、デバイス保護ドメイン
  - 成果物: `src/iommu/vtd.rs`, `src/iommu/amdv.rs`
  - 工数: 大

### フェーズ5: 管理プレーン最小機能
- [ ] タスク: シリアル/UEFIコンソール経由CLI（最小）
  - 成果物: `src/ctl/cli.rs`
  - 工数: 中
- [ ] タスク: VM作成/起動/停止/削除の基本API
  - 成果物: `src/hv/vm.rs`, `src/hv/vcpu.rs`
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
- [ ] タスク: 構造化ログ（レベル/カテゴリ/言語タグ）
  - 成果物: `src/obs/log.rs`（設計）、ログフォーマット仕様書
  - 工数: 中
- [ ] タスク: メトリクス（カウンタ/ヒストグラム）
  - 成果物: `src/obs/metrics.rs`（設計）、導入箇所一覧
  - 工数: 中
- [ ] タスク: トレース（VM‑Entry/Exit、EPT操作）
  - 成果物: `src/obs/trace.rs`（設計）
  - 工数: 中
- [ ] タスク: 多言語辞書（CLI/ログ）
  - 成果物: `lang/ja.json`, `lang/en.json`, `lang/zh.json`
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
  - 工数: 小
- [ ] タスク: SR‑IOV/ACS未対応デバイス検出と制限
  - 成果物: 要件とテスト項目
  - 工数: 小
