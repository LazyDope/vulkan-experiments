use std::ffi::CStr;

use ash::{extensions as ext, vk, Device, Entry, Instance};
use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle};
use winit::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder},
};

fn main() -> anyhow::Result<()> {
    let app = TutorApp::new()?;

    app.run()?;

    Ok(())
}

struct QueueIndexes {
    graphics: u32,
    present: u32,
}

impl QueueIndexes {
    /// Folding function used to test if a queue for both graphics and presenting exists, with a preference for queues that support both
    fn fold_into(
        surface_ext: &ext::khr::Surface,
        dev: vk::PhysicalDevice,
        khr_surface: vk::SurfaceKHR,
        mut acc: [Option<u32>; 2],
        queue_i: usize,
        queue: &vk::QueueFamilyProperties,
    ) -> Result<[Option<u32>; 2], [Option<u32>; 2]> {
        let queue_i = queue_i as u32;
        let mut both = false;
        if queue.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
            acc[0] = Some(queue_i);
            both = true;
        }

        if unsafe { surface_ext.get_physical_device_surface_support(dev, queue_i, khr_surface) }
            .unwrap()
        {
            acc[1] = Some(queue_i);
            if both {
                return Err(acc);
            }
        }
        Ok(acc)
    }

    fn as_array(&self) -> [u32; 2] {
        [self.graphics, self.present]
    }
}

struct SwapChainSupport {
    capabilities: vk::SurfaceCapabilitiesKHR,
    formats: Vec<vk::SurfaceFormatKHR>,
    present_modes: Vec<vk::PresentModeKHR>,
}

impl SwapChainSupport {
    unsafe fn new(
        surface: &ext::khr::Surface,
        device: vk::PhysicalDevice,
        khr_surface: vk::SurfaceKHR,
    ) -> anyhow::Result<Self> {
        Ok(SwapChainSupport {
            capabilities: surface.get_physical_device_surface_capabilities(device, khr_surface)?,
            formats: surface.get_physical_device_surface_formats(device, khr_surface)?,
            present_modes: surface
                .get_physical_device_surface_present_modes(device, khr_surface)?,
        })
    }

    fn choose_swap_surface_format(&self) -> vk::SurfaceFormatKHR {
        let mut formats: Vec<_> = self
            .formats
            .iter()
            .map(|format| {
                (
                    format,
                    (format.format == vk::Format::B8G8R8A8_SRGB) as u8
                        + (format.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR) as u8,
                )
            })
            .collect();
        formats.sort_by(|f1, f2| f1.1.cmp(&f2.1));
        *formats[0].0
    }

    const DESIRED_MODES: [vk::PresentModeKHR; 4] = [
        vk::PresentModeKHR::MAILBOX,
        vk::PresentModeKHR::IMMEDIATE,
        vk::PresentModeKHR::FIFO_RELAXED,
        vk::PresentModeKHR::FIFO,
    ];
    fn choose_swap_present_mode(&self) -> vk::PresentModeKHR {
        *Self::DESIRED_MODES
            .iter()
            .filter(|mode| self.present_modes.contains(mode))
            .next()
            .expect("FIFO should be guaranteed to exist")
    }

    fn get_swap_extent(&self, window: &Window) -> vk::Extent2D {
        let caps = self.capabilities;
        if caps.current_extent.width != u32::MAX {
            caps.current_extent
        } else {
            let inner = window.inner_size();

            vk::Extent2D::builder()
                .width(
                    inner
                        .width
                        .clamp(caps.min_image_extent.width, caps.max_image_extent.width),
                )
                .height(
                    inner
                        .height
                        .clamp(caps.min_image_extent.height, caps.max_image_extent.height),
                )
                .build()
        }
    }
}

/// Convert to cstr at compile time
const fn into_cstr(value: &str) -> &CStr {
    match CStr::from_bytes_until_nul(value.as_bytes()) {
        Ok(val) => val,
        Err(_) => panic!("Invalid CStr from str"),
    }
}

macro_rules! cstr {
    ( $val:literal ) => {
        $crate::into_cstr(concat!($val, "\0"))
    };
}

struct TutorApp {
    event_loop: Option<EventLoop<()>>,
    window: Window,

    entry: Entry,
    instance: Instance,
    surface_ext: ext::khr::Surface,
    surface_khr: vk::SurfaceKHR,

    physical_device: vk::PhysicalDevice,
    device: Device,

    graphics_queue: vk::Queue,
    present_queue: vk::Queue,

    swapchain_ext: ext::khr::Swapchain,
    swapchain: vk::SwapchainKHR,
    swapchain_images: Vec<vk::Image>,
    format: vk::Format,
    extent: vk::Extent2D,

    swapchain_image_views: Vec<vk::ImageView>,
}

impl TutorApp {
    const DEVICE_EXTENSIONS: [&'static CStr; 1] = [cstr!("VK_KHR_swapchain")];

    pub fn new() -> anyhow::Result<Self> {
        let (event_loop, window) = Self::init_window();
        let (
            entry,
            instance,
            surface_ext,
            surface_khr,
            physical_device,
            device,
            graphics_queue,
            present_queue,
            swapchain_ext,
            swapchain,
            swapchain_images,
            format,
            extent,
            swapchain_image_views,
        ) = Self::init_vulkan(&window)?;
        Ok(Self {
            window,
            event_loop: Some(event_loop),

            entry,
            instance,
            surface_ext,
            surface_khr,

            physical_device,
            device,

            graphics_queue,
            present_queue,

            swapchain_ext,
            swapchain,
            swapchain_images,
            format,
            extent,

            swapchain_image_views,
        })
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

    fn init_vulkan(
        window: &Window,
    ) -> anyhow::Result<(
        Entry,
        Instance,
        ext::khr::Surface,
        vk::SurfaceKHR,
        vk::PhysicalDevice,
        Device,
        vk::Queue,
        vk::Queue,
        ext::khr::Swapchain,
        vk::SwapchainKHR,
        Vec<vk::Image>,
        vk::Format,
        vk::Extent2D,
        Vec<vk::ImageView>,
    )> {
        let (entry, instance, rdh) = Self::create_instance(window)?;
        let surface_ext = ext::khr::Surface::new(&entry, &instance);

        let surface_khr = unsafe {
            ash_window::create_surface(&entry, &instance, rdh, window.raw_window_handle(), None)?
        };

        let (physical_device, queue_ids) = Self::pick_device(&instance, &surface_ext, surface_khr)?;

        let (device, graphics_queue, present_queue) =
            Self::create_logical_device(&instance, physical_device, &queue_ids)?;

        let swapchain_ext = ext::khr::Swapchain::new(&instance, &device);

        let (swapchain, swapchain_images, format, extent) = Self::create_swapchain(
            &surface_ext,
            window,
            &swapchain_ext,
            physical_device,
            surface_khr,
            &queue_ids,
        )?;

        let swapchain_image_views = Self::create_image_views(&device, &swapchain_images, format)?;

        Ok((
            entry,
            instance,
            surface_ext,
            surface_khr,
            physical_device,
            device,
            graphics_queue,
            present_queue,
            swapchain_ext,
            swapchain,
            swapchain_images,
            format,
            extent,
            swapchain_image_views,
        ))
    }
    fn create_instance(window: &Window) -> anyhow::Result<(Entry, Instance, RawDisplayHandle)> {
        let entry = Entry::linked();
        let app_info = vk::ApplicationInfo::builder().api_version(vk::make_api_version(0, 1, 0, 0));
        let rdh = window.raw_display_handle();
        let exts = ash_window::enumerate_required_extensions(rdh)?;
        let create_info = vk::InstanceCreateInfo::builder()
            .application_info(&app_info)
            .enabled_extension_names(exts);
        let instance = unsafe { entry.create_instance(&create_info, None)? };
        Ok((entry, instance, rdh))
    }

    fn pick_device(
        instance: &Instance,
        surface_ext: &ext::khr::Surface,
        khr_surface: vk::SurfaceKHR,
    ) -> anyhow::Result<(vk::PhysicalDevice, QueueIndexes)> {
        let devices = unsafe { instance.enumerate_physical_devices()? };

        let (_, &device, queue_ids) = devices
            .iter()
            .filter_map(|dev| {
                let mut score = 0;

                let queues = unsafe { instance.get_physical_device_queue_family_properties(*dev) };

                let queue_ids = match queues.iter().enumerate().try_fold(
                    [None, None],
                    |acc, (queue_i, queue)| {
                        QueueIndexes::fold_into(
                            &surface_ext,
                            *dev,
                            khr_surface,
                            acc,
                            queue_i,
                            queue,
                        )
                    },
                ) {
                    Ok([Some(graphics), Some(present)]) => QueueIndexes { graphics, present },
                    Err([Some(graphics), Some(present)]) => QueueIndexes { graphics, present },
                    _ => return None,
                };

                let props = unsafe { instance.get_physical_device_properties(*dev) };

                let extensions: Vec<&CStr> =
                    unsafe { instance.enumerate_device_extension_properties(*dev) }
                        .ok()?
                        .iter()
                        .map(|prop| unsafe { CStr::from_ptr(prop.extension_name.as_ptr()) })
                        .collect();
                if Self::DEVICE_EXTENSIONS
                    .iter()
                    .any(|ext| !extensions.contains(ext))
                {
                    return None;
                }

                let swapchain_support =
                    unsafe { SwapChainSupport::new(&surface_ext, *dev, khr_surface).ok()? };
                if swapchain_support.formats.is_empty()
                    && swapchain_support.present_modes.is_empty()
                {
                    return None;
                }

                score += props.limits.max_image_dimension2_d;

                if score > 0 {
                    Some((score, dev, queue_ids))
                } else {
                    None
                }
            })
            .max_by(|(score1, ..), (score2, ..)| score1.cmp(score2))
            .expect("Failed to find a suitable GPU");

        Ok((device, queue_ids))
    }

    fn create_logical_device(
        instance: &Instance,
        device: vk::PhysicalDevice,
        queue_ids: &QueueIndexes,
    ) -> anyhow::Result<(Device, vk::Queue, vk::Queue)> {
        let queue_priorities = [1.];

        let mut queue_info = vec![vk::DeviceQueueCreateInfo::builder()
            .queue_family_index(queue_ids.graphics)
            .queue_priorities(&queue_priorities)
            .build()];

        if queue_ids.graphics != queue_ids.present {
            queue_info.push(
                vk::DeviceQueueCreateInfo::builder()
                    .queue_family_index(queue_ids.present)
                    .queue_priorities(&queue_priorities)
                    .build(),
            )
        }

        let exts = Self::DEVICE_EXTENSIONS.map(|str| str.as_ptr());
        let features = vk::PhysicalDeviceFeatures::default();
        let device_create_info = vk::DeviceCreateInfo::builder()
            .queue_create_infos(&queue_info)
            .enabled_extension_names(&exts)
            .enabled_features(&features);

        let device = unsafe { instance.create_device(device, &device_create_info, None)? };

        let graphics_queue = unsafe { device.get_device_queue(queue_ids.graphics, 0) };
        let present_queue = unsafe { device.get_device_queue(queue_ids.present, 0) };

        Ok((device, graphics_queue, present_queue))
    }

    fn create_swapchain(
        surface_ext: &ext::khr::Surface,
        window: &Window,
        swapchain_ext: &ext::khr::Swapchain,
        physical_device: vk::PhysicalDevice,
        khr_surface: vk::SurfaceKHR,
        queue_ids: &QueueIndexes,
    ) -> anyhow::Result<(vk::SwapchainKHR, Vec<vk::Image>, vk::Format, vk::Extent2D)> {
        let sc_support =
            unsafe { SwapChainSupport::new(surface_ext, physical_device, khr_surface)? };

        let image_count = {
            let curr = sc_support.capabilities.min_image_count + 1;
            if (sc_support.capabilities.max_image_count > 0)
                && (curr > sc_support.capabilities.max_image_count)
            {
                sc_support.capabilities.max_image_count
            } else {
                curr
            }
        };
        let surface_format = sc_support.choose_swap_surface_format();
        let present = sc_support.choose_swap_present_mode();
        let extent = sc_support.get_swap_extent(window);

        let builder = vk::SwapchainCreateInfoKHR::builder()
            .surface(khr_surface)
            .min_image_count(image_count)
            .image_format(surface_format.format)
            .image_color_space(surface_format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .pre_transform(sc_support.capabilities.current_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(present)
            .old_swapchain(vk::SwapchainKHR::null());

        let q_ids = queue_ids.as_array();
        let swapchain_info = if queue_ids.graphics == queue_ids.present {
            builder.image_sharing_mode(vk::SharingMode::EXCLUSIVE)
        } else {
            builder
                .image_sharing_mode(vk::SharingMode::CONCURRENT)
                .queue_family_indices(&q_ids)
        };
        let swapchain = unsafe { swapchain_ext.create_swapchain(&swapchain_info, None)? };
        let swapchain_images = unsafe { swapchain_ext.get_swapchain_images(swapchain)? };

        Ok((swapchain, swapchain_images, surface_format.format, extent))
    }

    fn create_image_views(
        device: &Device,
        images: &Vec<vk::Image>,
        format: vk::Format,
    ) -> anyhow::Result<Vec<vk::ImageView>> {
        images
            .iter()
            .map(|image| {
                let image_info = vk::ImageViewCreateInfo::builder()
                    .image(*image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format)
                    .components(
                        vk::ComponentMapping::builder()
                            .r(vk::ComponentSwizzle::IDENTITY)
                            .g(vk::ComponentSwizzle::IDENTITY)
                            .b(vk::ComponentSwizzle::IDENTITY)
                            .a(vk::ComponentSwizzle::IDENTITY)
                            .r(vk::ComponentSwizzle::IDENTITY)
                            .build(),
                    )
                    .subresource_range(
                        vk::ImageSubresourceRange::builder()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .base_mip_level(0)
                            .level_count(1)
                            .base_array_layer(0)
                            .layer_count(1)
                            .build(),
                    );

                Ok(unsafe { device.create_image_view(&image_info, None)? })
            })
            .collect()
    }
}

impl TutorApp {
    pub fn run(mut self) -> anyhow::Result<()> {
        self.main_loop()?;
        Ok(())
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
}

impl Drop for TutorApp {
    fn drop(&mut self) {
        unsafe {
            for image in &self.swapchain_image_views {
                self.device.destroy_image_view(*image, None)
            }
            self.swapchain_ext.destroy_swapchain(self.swapchain, None);
            self.device.destroy_device(None);

            self.surface_ext.destroy_surface(self.surface_khr, None);
            self.instance.destroy_instance(None);
        }
    }
}
