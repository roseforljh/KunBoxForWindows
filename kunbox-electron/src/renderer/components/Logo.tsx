import React from 'react';
import { motion } from 'framer-motion';

interface LogoProps {
  className?: string;
  size?: number;
  animated?: boolean;
}

export const Logo: React.FC<LogoProps> = ({ 
  className = '', 
  size = 32,
  animated = true 
}) => {
  const outerLineTransition = {
    duration: 1.5,
    ease: "easeInOut" as const,
    delay: 0.2
  };

  const innerLineTransition = {
    duration: 1.2,
    ease: "easeOut" as const,
    delay: 0.8
  };

  return (
    <div 
      className={`relative inline-flex items-center justify-center ${className}`} 
      style={{ width: size, height: size }}
    >
      <svg 
        viewBox="0 0 100 100" 
        fill="none" 
        xmlns="http://www.w3.org/2000/svg"
        className="w-full h-full"
      >
        <g 
          stroke="currentColor" 
          strokeWidth="8" 
          strokeLinejoin="round" 
          strokeLinecap="round"
        >
          {/* Outer Hexagon */}
          <motion.path 
            d="M 50 22 L 74 36 L 74 64 L 50 78 L 26 64 L 26 36 Z" 
            initial={animated ? { pathLength: 0, opacity: 0 } : false}
            animate={{ pathLength: 1, opacity: 1 }}
            transition={outerLineTransition}
          />
          {/* Inner Y */}
          <motion.path 
            d="M 50 50 L 50 78" 
            initial={animated ? { pathLength: 0, opacity: 0 } : false}
            animate={{ pathLength: 1, opacity: 1 }}
            transition={innerLineTransition}
          />
          <motion.path 
            d="M 50 50 L 26 36" 
            initial={animated ? { pathLength: 0, opacity: 0 } : false}
            animate={{ pathLength: 1, opacity: 1 }}
            transition={innerLineTransition}
          />
          <motion.path 
            d="M 50 50 L 74 36" 
            initial={animated ? { pathLength: 0, opacity: 0 } : false}
            animate={{ pathLength: 1, opacity: 1 }}
            transition={innerLineTransition}
          />
        </g>
      </svg>
    </div>
  );
};

export default Logo;
