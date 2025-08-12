### プロジェクト名
Zerovisor — 世界最高性能・最高信頼のType‑1（ベアメタル）ハイパーバイザー

### ビジョンと目標
- 物理ハードウェア上で直接動作する堅牢なType‑1ハイパーバイザーを提供し、サーバ、エッジ、車載、産業用途までを単一コアで網羅する。
- 既存製品を超える低オーバーヘッド、確定的レイテンシ、強力な分離性と安全性、管理性を実現する。
- C/C++依存を完全排除し、Rust（pure Rust）を中核に据えた安全・高信頼実装を貫徹する。

### 対象ユーザー
- データセンター運用者、クラウド/オンプレ事業者
- エッジ/産業/車載システムインテグレータ（確定的制御と強い分離が必須）
- セキュリティセンシティブなワークロードの隔離を求めるユーザー

### 用語
- VM: 仮想マシン、VMM: 仮想マシンモニタ、HV: ハイパーバイザー
- VMX/EPT: Intel 仮想化、SVM/NPT: AMD 仮想化
- VT‑d/AMD‑VI: IOMMU（DMAリマップ）

### 機能要件（Functional Requirements）
- CPU仮想化
  - Intel VMX/EPT および AMD SVM/NPT をサポートし、機能が存在するプラットフォームで自動検出・最適化。
  - 仮想CPUスケジューリング（公平/重み付き/RTクラスの複合スケジューラ）。
  - APIC/HPET/TSC/タイムソースの仮想化、タイムキーピングの高精度化、vTimer。
- メモリ仮想化
  - 二段変換（EPT/NPT）によるMMU仮想化、巨大ページ最適化、NUMA認識の割当。
  - メモリバルーン、ページ共有（同一ページ検出）、暗号化（PG/W^X/SMEP/SMAP補助）。
- デバイス仮想化
  - パラバーチャル（VirtIO規格）: ネットワーク、ブロック、RNG、コンソール、GPU（将来拡張）
  - vPCI/vIOMMU、SR‑IOVパススルー、DMAリマップ（VT‑d/AMD‑VI）。
- ストレージ/ネットワーク
  - 低レイテンシI/Oパス、マルチキュー、RSS、ゼロコピー経路。
- セキュリティ/隔離
  - VM間強分離、IOMMU必須時の強制、メモリ初期化消去、KASLR相当、スタック/ヒープ堅牢化。
  - セキュアブート連携、測定ブート（TPMイベントログ活用）。
- 管理・可観測性
  - 管理面（Control Plane）はホスト外部ないし別CPUコアへ分離可能な設計。
  - ログ/メトリクス/トレース、CRIU相当のVMサスペンド/レジューム基盤。
- 可用性
  - VMライブマイグレーション、フェンシング、HAクラスタ連携（将来拡張含む）。
- 多言語化
  - 管理UI/CLI/ログのi18n（日本語/英語/中国語を最低限）。

### 非機能要件（Non‑Functional Requirements）
- パフォーマンス
  - vCPUスイッチ時のオーバーヘッド最小化、EPT/NPT TLBヒット率最大化、IPI/割込み負荷低減。
  - ゼロコピーI/O、NUMA最適配置、ヒュージページ活用。
- スケーラビリティ
  - 数百vCPU/VM規模、NUMAノード横断の効率的スケジューリング。
- セキュリティ
  - メモリ安全（Rust）、UB回避、境界チェック、攻撃面最小化、最小権限原則。
  - IOMMU有効時以外のデバイスパススルー禁止、未使用機能の無効化。
- 信頼性/可用性
  - フェイルファストと自己回復、ウォッチドッグ、クラッシュダンプ、堅牢なログ。
- 保守性/拡張性
  - 明確なモジュール境界、安定した内外部API、テスト容易性。
- ユーザビリティ
  - 一貫したCLI/REST、役割ベースアクセス制御、監査ログ。
- 実行環境
  - UEFIブート、x86‑64アーキテクチャ、SMP、ACPI、APIC/MSI‑X。
  - 管理面は別ホスト/別コア/別VMのいずれにも配置可能。

### 参照仕様（一次情報）
- Intel SDM（VMX/EPT）: [Intel 64 and IA‑32 Architectures Software Developer’s Manual](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html)
- AMD APM（SVM/NPT）: [AMD64 Architecture Programmer’s Manual](https://www.amd.com/en/developer/tech-docs.html)
- UEFI: [UEFI Specification](https://uefi.org/specifications)
- ACPI: [ACPI Specification](https://uefi.org/specifications)
- Intel VT‑d: [Intel Virtualization Technology for Directed I/O](https://www.intel.com/content/www/us/en/content-details/671488/intel-virtualization-technology-for-directed-i-o-architecture-specification.html)
- AMD‑VI: [AMD IOMMU Architecture Specification](https://www.amd.com/en/developer/tech-docs.html)

### 制約事項
- C/C++言語およびそれらでコンパイルされたバイナリに依存するライブラリは不使用。
- カーネル中核はRustのみで構築（pure Rust）。
- ベンダ固有機能差異は抽象層で吸収しつつ、存在検出により最適化経路を自動選択。

### 受入基準（例）
- UEFI上で起動し、CPU仮想化能力（VMX/SVM、EPT/NPT、VT‑d/AMD‑VI）の検出結果を表示できること。
- VMCTL（管理面）からVMの生成/起動/停止/削除の基本操作が行えること（段階的に拡張）。
- IOMMU有効化時、デバイスパススルーを安全に実行可能であること。

### 互換性と対象範囲（Compatibility Matrix）
- CPU: Intel VMX/EPT、AMD SVM/NPT を検出し自動選択。APICv/Posted‑Interrupts または AVIC を可能なら使用。
- IOMMU: VT‑d（インテル）/ AMD‑VI（AMD）。ATS/PRI/PASID は対応HWでのみ有効化。
- ブート: UEFI 2.x 準拠環境（CSM非依存）。

### 観測性と運用（Observability & Operations）
- ログ: 構造化ログ（JSONライン等）、重大度/カテゴリ/言語タグ。ローテーションと堅牢な永続化。
- メトリクス: vCPUスケジュール時間、VM‑Entry/Exit回数、EPT/NPTインバリデーション、I/Oキュー深さ。
- トレース: VM‑Entry/Exit、割込注入、EPT更新、IOMMUマップ変更。

### セキュリティ要求（Security Requirements）
- メモリ安全（Rust）、W^X、SMEP/SMAP、未初期化の禁止。
- 測定ブート連携（TPMイベントログ）。IOMMU無しのパススルーを禁止。
- 鍵/秘密の作用域最小化、使用後ゼロ化、監査ログ非記録。

### パフォーマンス要求（Performance Targets）
- VM‑Entry/Exit の平均/分位の上限値を規定（実測ベースで更新）。
- EPT/NPTヒット率、TLBシュートダウン頻度、Posted‑Interrupts/AVIC適用比率のKPIを設定。
- I/Oゼロコピー率とスループット/レイテンシSLOを定義。

### 多言語化（Internationalization）
- 管理UI/CLI/ログは日/英/中を最低限サポート。辞書欠落時は英語へフォールバック。

### リスクと対策（Risks & Mitigations）
- ハードウェア差異: 検出と機能降格（graceful degradation）。
- タイマ精度: Invariant TSCが不可の場合HPETへフォールバックし、ドリフト補正を実施。
- 互換性: SR‑IOV/ACS未対応デバイスはパススルー制限。

### 追加参照
- APICv/Posted‑Interrupts / Invariant TSC: [Intel SDM](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html)
- AVIC / SVM: [AMD Tech Docs](https://www.amd.com/en/developer/tech-docs.html)
- UEFI/ACPI: [UEFI Specifications](https://uefi.org/specifications)
- VT‑d: [Intel VT‑d Architecture Specification](https://www.intel.com/content/www/us/en/content-details/671488/intel-virtualization-technology-for-directed-i-o-architecture-specification.html)
- AMD‑VI: [AMD IOMMU Architecture Specification](https://www.amd.com/en/developer/tech-docs.html)
