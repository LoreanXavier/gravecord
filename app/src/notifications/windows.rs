//! WinRT toast delivery on a dedicated apartment. Capture runs never construct this worker.
use super::taskbar::Taskbar;
use super::{APP_ID, Command, Notice, toast_xml};
use crate::net::Update;
use std::{
    collections::{HashMap, VecDeque},
    sync::mpsc::Receiver,
};
use tokio::sync::mpsc::UnboundedSender;
use windows::{
    Data::Xml::Dom::XmlDocument,
    Foundation::TypedEventHandler,
    UI::Notifications::{ToastNotification, ToastNotificationManager, ToastNotifier},
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    core::HSTRING,
};

fn show(
    notifier: &ToastNotifier,
    notice: Notice,
    updates: &UnboundedSender<Update>,
) -> windows::core::Result<ToastNotification> {
    let xml = XmlDocument::new()?;
    xml.LoadXml(&HSTRING::from(toast_xml(&notice.title, &notice.body)))?;
    let toast = ToastNotification::CreateToastNotification(&xml)?;
    toast.SetTag(&HSTRING::from(format!("{:x}", notice.message)))?;
    toast.SetGroup(&HSTRING::from(format!("{:x}", notice.channel)))?;
    let updates = updates.clone();
    let channel = notice.channel;
    toast.Activated(&TypedEventHandler::new(move |_, _| {
        let _ = updates.send(Update::NotificationClicked(channel));
        Ok(())
    }))?;
    notifier.Show(&toast)?;
    Ok(toast)
}

pub(super) fn run(
    commands: Receiver<Command>,
    updates: UnboundedSender<Update>,
    generation: std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    // This thread owns its WinRT apartment and all retained toast objects.
    if unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_err() {
        tracing::warn!("notification apartment initialization failed");
        return;
    }
    {
        let notifier =
            ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID)).ok();
        if notifier.is_none() {
            tracing::warn!("desktop toast notifier unavailable");
        }
        if let Ok(history) = ToastNotificationManager::History() {
            let _ = history.ClearWithId(&HSTRING::from(APP_ID));
        }
        let mut shown: HashMap<u64, VecDeque<ToastNotification>> = HashMap::new();
        let mut taskbar: Option<Taskbar> = None;
        let mut applied_generation = 0;
        for command in commands {
            let current_generation = generation.load(std::sync::atomic::Ordering::Acquire);
            if current_generation != applied_generation
                && let Some(taskbar) = &mut taskbar
            {
                match taskbar.ready() {
                    Ok(()) => applied_generation = current_generation,
                    Err(error) => {
                        tracing::warn!(code=?error.code(), "taskbar initialization failed")
                    }
                }
            }
            match command {
                Command::Window(handle) => {
                    taskbar = Some(Taskbar::new(handle));
                    applied_generation = 0;
                }
                Command::TaskbarReady => {}
                Command::Badge(count) => {
                    if let Some(taskbar) = &mut taskbar
                        && let Err(error) = taskbar.badge(count)
                    {
                        tracing::warn!(code=?error.code(), "taskbar badge failed");
                    }
                }
                Command::Flash(enabled) => {
                    if let Some(taskbar) = &taskbar {
                        taskbar.flash(enabled);
                    }
                }
                Command::Show(notice) => {
                    let Some(notifier) = &notifier else {
                        continue;
                    };
                    let channel = notice.channel;
                    match show(notifier, notice, &updates) {
                        Ok(toast) => {
                            let queue = shown.entry(channel).or_default();
                            queue.push_back(toast);
                            while queue.len() > 20 {
                                if let Some(old) = queue.pop_front() {
                                    let _ = notifier.Hide(&old);
                                }
                            }
                        }
                        Err(error) => {
                            tracing::warn!(code=?error.code(), "desktop notification failed")
                        }
                    }
                }
                Command::Clear(channel) => {
                    let Some(notifier) = &notifier else {
                        continue;
                    };
                    if let Some(queue) = shown.remove(&channel) {
                        for toast in queue {
                            let _ = notifier.Hide(&toast);
                        }
                    }
                }
            }
        }
    }
    unsafe { RoUninitialize() };
}
