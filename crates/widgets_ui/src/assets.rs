use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

gpui_kit::assets::icon_assets!(
    DesignIcons,
    [
        ArrowLeft,
        ArrowRight,
        ArrowUpRight,
        Bell,
        Bookmark,
        Bot,
        Check,
        ChevronDown,
        ChevronRight,
        Circle,
        CircleAlert,
        CircleCheck,
        CircleDot,
        CircleX,
        Clock,
        Code,
        Command,
        Copy,
        Ellipsis,
        ExternalLink,
        Eye,
        File,
        FileCode,
        FileDiff,
        Folder,
        FolderGit2,
        GitBranch,
        GitCommitHorizontal,
        GitCompare,
        GitMerge,
        GitPullRequest,
        GitPullRequestClosed,
        GitPullRequestDraft,
        Github,
        Inbox,
        KeyRound,
        ListFilter,
        ListTree,
        LoaderCircle,
        Lock,
        LogOut,
        MessageSquare,
        MessageSquarePlus,
        Minus,
        PanelLeft,
        PanelRight,
        Pencil,
        Play,
        Plus,
        RefreshCw,
        Reply,
        RotateCcw,
        Search,
        Send,
        Settings,
        Sparkles,
        Square,
        SquareCheck,
        Tag,
        Terminal,
        User,
        Users,
        X,
        Keyboard,
        Palette,
        Trash,
        Link,
        UserPlus,
        Maximize2,
        Minimize2,
    ]
);

/// The application's asset source: the icons the design uses, falling back to
/// gpui-kit's default component bundle. Register it with `with_assets`.
#[derive(Clone, Copy, Debug, Default)]
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = DesignIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(DesignIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
