use crate::{Backend, Renderer, Settings};

use iced_graphics::Viewport;
use iced_native::{futures, mouse};
use raw_window_handle::HasRawWindowHandle;
use std::iter;

/// A window graphics backend for iced powered by `wgpu`.
pub struct Compositor {
    settings: Settings,
    instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl std::fmt::Debug for Compositor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Compositor {{}}")
    }
}

impl Compositor {
    /// Requests a new [`Compositor`] with the given [`Settings`].
    ///
    /// Returns `None` if no compatible graphics adapter could be found.
    ///
    /// [`Compositor`]: struct.Compositor.html
    /// [`Settings`]: struct.Settings.html
    pub async fn request(settings: Settings) -> Option<Self> {
        let instance = wgpu::Instance::new();

        let adapter = instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: if settings.antialiasing.is_none() {
                    wgpu::PowerPreference::Default
                } else {
                    wgpu::PowerPreference::HighPerformance
                },
                compatible_surface: None,
            },
            wgpu::UnsafeExtensions::disallow(),
            wgpu::BackendBit::PRIMARY,
        )
        .await?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    extensions: wgpu::Extensions::empty(),
                    limits: wgpu::Limits { max_bind_groups: 2, ..Default::default() },
                    shader_validation: true,
                },
                None,
            ).await.ok()?;

        Some(Compositor {
            settings,
            instance,
            device,
            queue,
        })
    }

    /// Creates a new rendering [`Backend`] for this [`Compositor`].
    ///
    /// [`Compositor`]: struct.Compositor.html
    /// [`Backend`]: struct.Backend.html
    pub fn create_backend(&self) -> Backend {
        Backend::new(&self.device, self.settings)
    }
}

impl iced_graphics::window::Compositor for Compositor {
    type Settings = Settings;
    type Renderer = Renderer;
    type Surface = wgpu::Surface;
    type SwapChain = wgpu::SwapChain;

    fn new(settings: Self::Settings) -> (Self, Renderer) {
        let compositor = futures::executor::block_on(Self::request(settings))
            .expect("Could not find a suitable graphics adapter");

        let backend = compositor.create_backend();

        (compositor, Renderer::new(backend))
    }

    fn create_surface<W: HasRawWindowHandle>(
        &mut self,
        window: &W,
    ) -> wgpu::Surface {
        #[allow(unsafe_code)]
        unsafe { self.instance.create_surface(window) }
    }

    fn create_swap_chain(
        &mut self,
        surface: &Self::Surface,
        width: u32,
        height: u32,
    ) -> Self::SwapChain {
        self.device.create_swap_chain(
            surface,
            &wgpu::SwapChainDescriptor {
                usage: wgpu::TextureUsage::OUTPUT_ATTACHMENT,
                format: self.settings.format,
                width,
                height,
                present_mode: wgpu::PresentMode::Mailbox,
            },
        )
    }

    fn draw<T: AsRef<str>>(
        &mut self,
        renderer: &mut Self::Renderer,
        swap_chain: &mut Self::SwapChain,
        viewport: &Viewport,
        output: &<Self::Renderer as iced_native::Renderer>::Output,
        overlay: &[T],
    ) -> mouse::Interaction {
        let frame = swap_chain.get_next_frame().expect("Next frame");

        let mut encoder = self.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: None },
        );

        let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                attachment: &frame.output.view,
                resolve_target: None,
                load_op: wgpu::LoadOp::Clear,
                store_op: wgpu::StoreOp::Store,
                clear_color: wgpu::Color {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                },
            }],
            depth_stencil_attachment: None,
        });

        let mouse_interaction = renderer.backend_mut().draw(
            &mut self.device,
            &mut encoder,
            &frame.output.view,
            viewport,
            output,
            overlay,
        );

        self.queue.submit(iter::once(encoder.finish()));

        mouse_interaction
    }
}
