# 多天體實作設計

世界描述置於無 Bevy 的 `void-fleet-flight::world`。`WorldDescription` 保存 SystemSpec 和以穩定字串 ID 為 key 的 BodyDescription；BodyDescription 有 optional terrain、明確 air density/datum、sea、VisualSettings。`InitialWorld` 保存 world、launch_body、craft、launch_site，不保存 renderer handles。单天體 preset 經明確 constructor 包裝，無 schema fallback。

`WorldDescription::build` 建立同一 ephemeris、Environment、每個有 terrain 的 GroundSpec；實際環境與 renderer 都讀同一 descriptor。初始化和 restore 共用它。重複／未知 ID、地形半徑不合、非法視覺及環境數字直接 panic。

home 分類：launch/new/LaunchGround 是發射身份；predict 和 simple camera surface axes 改選當前 navigation/nearest configured terrain；正式主遊戲 renderer 暫維持單天體模式。獨立 multi_body example 管理正式 FleetFlight 的多天體畫面，避免未驗收模式成為預設。

近景只啟用 camera/selected ship 附近一顆天體，遠景球永遠依 frame tree 有正確位置與尺寸。每天體獨立 TileField、material 和 mesh accounting；卸載 drop Task，舊工作不能跨 field/generation 交付。reset/load 建新 scene generation，舊 entities/assets 明確 despawn/remove。Collider 身份使用 body index+origin，避免兩地同 origin 碰撞。物理 owner/collider 與 renderer 完全獨立。

Checkpoint 重建完整 world，world_mark 保存 world descriptor及各 body stable ID；MODEL_VERSION 暫 12（另一 branch 暫10，最後由主審分配）。新命令 LaunchGroundAt/LaunchOrbitAt 明確 body ID，錄放記錄 fixture/focus。既有 LaunchGround 定義保持 launch body。

第一成果：同世界 Aurelia 与 Luna 各有船，Tab 切船，各体地形/碰撞與近遠景；降落 fixture 和真實轉移測試再補。無大氣月球顯式停用海/云/散射。第一版限制只有一套近景大氣合成、不處理多恆星/eclipses。

實作細節更新：renderer cache現在放獨立 `app::multi_body`，完整descriptor變更清理各体textures/materials/far spheres；相同descriptor的reset/load以 terrain Arc instance 判斷generation變更，清掉pending map並重建field。只有單一近景LOD啟用，遠景球在LOD尚未可繪時維持；每次generation重建會釋放app持有mesh。現有 TileField新增unload且despawn同步remove自己建立的mesh；不改LOD算法。stateful module分支只需要合併Action局部新增與world_mark部分字段。

後續依使用者要求擴充：world schema 2，序列化自訂 optics/cloud profile，多顆分離大氣的 HDR passes；歧義深度／相穿體積拒絕。詳見 [solar-scenery.md](solar-scenery.md)。
