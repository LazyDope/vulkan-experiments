use std::ffi::{CStr, CString};

use ash::{vk, Device, Entry, Instance};
use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
use winit::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder},
};

fn main() -> anyhow::Result<()> {
    let mut app = TutorApp::new()?;

    app.run()?;

    Ok(())
}

struct TutorApp {
    window: Window,
    event_loop: Option<EventLoop<()>>,
    _entry: Entry,
    instance: Instance,
    _surface: vk::SurfaceKHR,
    device: Device,
}

impl TutorApp {
    pub fn new() -> anyhow::Result<Self> {
        let (event_loop, window) = Self::init_window();
        let (_entry, instance, _surface, device) = Self::init_vulkan(&window)?;
        Ok(Self {
            window,
            event_loop: Some(event_loop),
            _entry,
            instance,
            _surface,
            device,
        })
    }
    pub fn run(&mut self) -> anyhow::Result<()> {
        self.main_loop()?;
        self.cleanup();
        Ok(())
    }

    fn init_window() -> (EventLoop<()>, Window) {
        let event_loop = EventLoop::new().unwrap();
        let window = WindowBuilder::new()
            .with_inner_size(LogicalSize::new(800, 600))
            .with_title("Hello Vulkan!")
            .build(&event_loop)
            .unwrap();

        event_loop.set_control_flow(ControlFlow::Poll);
        (event_loop, window)
    }

    fn init_vulkan(window: &Window) -> anyhow::Result<(Entry, Instance, vk::SurfaceKHR, Device)> {
        let entry = Entry::linked();
        let app_info = vk::ApplicationInfo {
            api_version: vk::make_api_version(0, 1, 0, 0),
            ..Default::default()
        };
        let rdh = window.raw_display_handle();
        let exts = ash_window::enumerate_required_extensions(rdh)?;
        let create_info = vk::InstanceCreateInfo::builder()
            .application_info(&app_info)
            .enabled_extension_names(exts)
            .build();
        let instance = unsafe { entry.create_instance(&create_info, None)? };

        let surface = unsafe {
            ash_window::create_surface(&entry, &instance, rdh, window.raw_window_handle(), None)?
        };

        let (device, queue_i, features) = Self::pick_device(&instance)?;

        let logical_device =
            Self::create_logical_device(&instance, device, queue_i, features, exts)?;

        Ok((entry, instance, surface, logical_device))
    }

    fn pick_device(
        instance: &Instance,
    ) -> anyhow::Result<(vk::PhysicalDevice, u32, vk::PhysicalDeviceFeatures)> {
        let devices = unsafe { instance.enumerate_physical_devices()? };

        let (_, &device, queue_i, feats) = devices
            .iter()
            .filter_map(|dev| {
                let mut score = 0;
                let feats = unsafe { instance.get_physical_device_features(*dev) };

                if feats.geometry_shader == 0 {
                    return None;
                }

                let queues = unsafe { instance.get_physical_device_queue_family_properties(*dev) };

                let Some((queue_i, _)) = queues
                    .iter()
                    .enumerate()
                    .filter(|(_, queue)| queue.queue_flags.contains(vk::QueueFlags::GRAPHICS))
                    .last()
                else {
                    return None;
                };

                let props = unsafe { instance.get_physical_device_properties(*dev) };

                if props.device_type == vk::PhysicalDeviceType::DISCRETE_GPU {
                    score += 1000;
                }

                score += props.limits.max_image_dimension2_d;

                if score > 0 {
                    Some((score, dev, queue_i as u32, feats))
                } else {
                    None
                }
            })
            .max_by(|(score1, ..), (score2, ..)| score1.cmp(score2))
            .expect("Failed to find a suitable GPU");

        Ok((device, queue_i, feats))
    }

    fn create_logical_device(
        instance: &Instance,
        device: vk::PhysicalDevice,
        queue_i: u32,
        features: vk::PhysicalDeviceFeatures,
        exts: &[*const i8],
    ) -> anyhow::Result<Device> {
        let queue_create_info = vk::DeviceQueueCreateInfo::builder()
            .queue_family_index(queue_i)
            .queue_priorities(&[1.0])
            .build();

        let device_create_info = vk::DeviceCreateInfo::builder()
            .queue_create_infos(&[queue_create_info])
            .enabled_features(&features)
            .enabled_extension_names(exts)
            .build();

        Ok(unsafe { instance.create_device(device, &device_create_info, None)? })
    }

    fn main_loop(&mut self) -> anyhow::Result<()> {
        self.event_loop
            .take()
            .unwrap()
            .run(|event, elwt| match event {
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => {
                    println!("Closing!");
                    elwt.exit();
                }
                Event::AboutToWait => {
                    self.window.request_redraw();
                }
                Event::WindowEvent {
                    event: WindowEvent::RedrawRequested,
                    ..
                } => {}
                _ => (),
            })?;
        Ok(())
    }

    fn cleanup(&mut self) {
        unsafe { self.instance.destroy_instance(None) };
    }
}
