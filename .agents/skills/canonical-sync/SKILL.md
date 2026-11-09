---
name: canonical-sync
description: Keep agent skills byte-identical across the workspace repositories by treating this repo (qoder-skills) as the canonical skill store, scanning every repo under ~/Workspaces for drift, and two-way merging the newer knowledge from sibling projects back into the canonical copy. Use when a skill has diverged between repos, when a project has improved a shared skill locally, or before distributing an updated skill to other projects.
---

# 技能庫同步（canonical-sync）

本倉庫 `qoder-skills/skills/` 為各專案共用技能之**正本源**。各專案之技能目錄
（`.agents/skills/`、`.opencode/skills/`、`skills/`）為其複本，內容應與正本源逐
元組相同，以便任何一庫皆可作為來源，且庫間差異比較恆為空白。

## 一、適用原則

1. **一名一檔**：同一技能在所有庫中僅有一份正典文字；不得因庫別而分叉。
2. **知識只加不減**：合併時須保留每一庫原有之知識要點，改寫措辭可，遺漏內容不可。
3. **專案專屬內容不進入正典**：測試指令與 sibling 技能連結等，應改為通用說法，或以
   「若該庫有此技能」之條件句式表述，使同一文字可通用。
4. **命名統一**：同一變數於各庫須同名（例如 Gitea 技能統一使用 `$GITEA_OWNER_REPO`）。
5. **僅拒外來修改**：目標庫若存有**未提交且非正典文字**之版本，則拒絕寫入；本工具
   `--apply` 所產生之正典內容雖未提交，仍屬可推送之正常狀態，不受此限。落後於其
   遠端者亦拒絕，須先處理分支。
6. **鋪展即新增**：`--repos` 指明一尚無該技能之庫時，於該庫自有之技能根目錄建立完整
   技能目錄（含 `scripts/` 等），而非略過。

## 二、判定新舊

檔案修改時間不足為憑，複製、checkout 均會改變它。依下列順序判定：

1. 以 `git log -1 --format=%ad -- <skill-path>` 取最後提交日期。
2. 日期相同者，比較內容涵蓋度：一檔多庫時，以「包含他人全部知識要點」者為新。
3. 若兩庫各有獨有內容，則為**雙向分叉**，須合併後再分發，不得單向複製。
4. 庫之遠端亦可能較其本地為新；比對前須 `git fetch` 並查看遠端之同檔內容。

## 三、掃描與彙報

列出某技能在各庫之狀態（路徑、大小、blob、髒淨、最近提交）：

```bash
python3 skills/canonical-sync/scripts/scan-skills.py --skill gitea-issues
```

全工作區之健檢（僅列出有差異者），供定期巡檢使用：

```bash
python3 skills/canonical-sync/scripts/scan-skills.py --only-drifted
python3 skills/canonical-sync/scripts/scan-skills.py --json
```

輸出樣例：

```text
gitea-issues — 7 copies, 4 variant(s)   DRIFT  (5 including uncommitted edits)
  rockbot    2026-11-09  21966B e8e8e78b clean .agents/skills/gitea-issues/SKILL.md
  weasel     2026-07-07   5444B d9747203 clean .agents/skills/gitea-issues/SKILL.md
```

- `variant(s)` 為**已提交**內容之種數；括號內為計入未提交修改後之種數。
- 標示 `unified` 或 `DRIFT`，依提交日期排序；`DIRTY` 表示該檔有未提交修改。
- 回傳碼：0 為一致，1 為有差異，2 為用法錯誤。
- 預設掃描根目錄為 `~/Workspaces`，可以 `--root` 更改。

## 四、合併程序

1. **選定正本源**：以知識最全之庫為 `--from`。若該庫非 `qoder-skills`，先將合併後之
   文字寫入 `qoder-skills/skills/<name>/`，使其成為正典。
2. **試算**（預設不寫入任何檔案）：

   ```bash
   python3 skills/canonical-sync/scripts/sync-skill.py <name> --from <repo> \
     --repos rockbot,weight-management-frontend,openrouter-image-runner
   ```

   輸出包含：正典文字之格式檢查結果、`coverage gaps`（目標仍持有而正典遺失之行）、
   各庫 `ahead/behind/dirty` 及將要做之動作。加上 `--fetch` 可先更新遠端參考再判落後。
3. **補足遺漏**：逐條檢視 `coverage gaps`，將屬實之知識併入正典，重試步驟 2，直至無
   遺漏。純措辭改寫而語意重複者，可判定為無損，續以 `--ignore-coverage`。
4. **寫入**：

   ```bash
   python3 skills/canonical-sync/scripts/sync-skill.py <name> --from qoder-skills --apply
   ```

5. **提交並推送**：提交僅限該技能目錄，勿夾帶其他檔案。

   ```bash
   python3 skills/canonical-sync/scripts/sync-skill.py <name> --from qoder-skills \
     --commit --push --message 'docs({skill}): adopt the cross-repo canonical skill text'
   ```

   `--commit` 已含寫入；被拒絕之庫不會被提交或推送。各庫提交訊息應依其慣例（本庫用
   繁體中文）。
6. **鋪展至新庫**：

   ```bash
   python3 skills/canonical-sync/scripts/sync-skill.py <name> --from qoder-skills \
     --repos projD --apply --commit --push
   ```

7. **多餘檔案**：目標技能目錄內正典所無之檔案會列示但不刪除；確認後加 `--prune`。

## 五、驗證

`--apply` 之後，腳本會計算各庫 `SKILL.md` 之 git blob，其數須**唯一**；不一致則列於
`BLOCKED`。亦可手工確認：

```bash
for p in rockbot qoder-skills openrouter-image-runner weight-management-frontend; do
  git -C ~/Workspaces/$p rev-parse HEAD:<skills-root>/<name>/SKILL.md
done | sort -u | wc -l   # 應為 1
```

格式檢查涵蓋：frontmatter 之 `name` 與目錄同名且為小寫連字號、`description` 不逾
1024 字元、程式碼圍欄成對、`bash` 區塊可透過 `bash -n`、無 CRLF、檔尾有換行。

## 六、測試

腳本之行為由一自足之測試套件驗證：於沙箱內建立四個（連鋪展目標共五個）暫時 git
倉庫，各附獨立之 bare remote，全程離線，不觸及沙箱外之任何倉庫。

```bash
python3 skills/canonical-sync/tests/sync-test.py --root ./tmp/canonical-sync-sandbox
```

涵蓋：漂移偵測、`coverage gaps`、髒淨與落後之拒絕、整目錄寫入、提交範圍、推送、
blob 唯一性、格式檢查、鋪展、用法錯誤。回傳碼 0 為全數通過，1 為有失敗；預設於結束
時清除沙箱，`--keep` 可保留。於禁用系統 `/tmp` 之環境，務必以 `--root` 指到本庫
`./tmp/`。

## 七、安全事項與常見故障

| 狀況 | 處置 |
| --- | --- |
| 目標庫該檔有未提交之外來修改 | 停止並告知使用者；確認後始用 `--allow-dirty` |
| 目標庫落後其遠端數百提交 | 不得推送。先 `--fetch` 比對遠端之同檔內容；若遠端已有獨立修改，須先將遠端內容一併納入正典，再由使用者決定 rebase 或另建分支 |
| 遠端回傳 502 或無法連線 | 提交留於本地，稍後再推；於回報中列明未推之庫 |
| 該庫以 git submodule 引入技能 | 此為外部來源（GitHub）安裝法，見 `install.md`；與本技能之本地正典同步無關，`skills-lock.json` 亦然 |
| 某庫之 sibling 技能（如 `issue-labels`、`gitea-bots`）採另一命名 | 該目錄為上游逐字同步之對象，得保留其原有寫法，不視為分叉 |
| 同步後 `coverage gaps` 大量列行 | 多為措辭或變數改名所致，依第二節步驟 2 判定語意是否仍屬遺漏 |
| 正典文字自身格式檢查不過 | 先修正典再分發；腳本於此情形一律拒絕寫入 |

## 八、其他

- 腳本僅使用 Python 標準庫（3.10+）及 `git`，無外部依賴。
- 中間產物一律置於本庫 `./tmp/`，不得使用系統 `/tmp`。
- 一次同步一個技能；全庫同步應逐項執行，以便各庫提交資訊清楚可追溯。
